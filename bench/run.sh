#!/usr/bin/env bash
# Run the rs-broker k6 benchmark.
#
#   bench/run.sh <tier> [scenario]
#
# tier:
#   100m-64m   producer and consumer capped at 100 millicores and 64 MiB
#   200m-128m  producer and consumer capped at 200 millicores and 128 MiB
#
# scenario:
#   single     one POST /api/v1/publish per request        (default)
#   batch      one POST /api/v1/publish/batch per request, BATCH_SIZE messages
#
# Kafka and Postgres run without limits on purpose: the benchmark measures
# the broker, not the infrastructure.
#
# k6 runs inside the benchmark's own container network, so no request pays
# a host-to-VM network hop and the benchmark publishes no host ports.
# Set BENCH_K6_HOST=1 to run k6 on the host instead (it then needs the
# broker's port published).
#
# Env overrides:
#   RATES       space separated request rates to sweep (default "50 100 200 400")
#   DURATION    test duration per rate        (default 30s)
#   VUS         pre-allocated VUs per rate    (default 64)
#   BATCH_SIZE  messages per batch call       (default 50)
#   SETTLE      idle seconds between rates    (default 20)
#   DRAIN_WAIT  max seconds to wait for the pipeline to settle (default 180)
#   KEEP=1      leave the stack running after the run
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$REPO_ROOT"

TIER=${1:-}
case "$TIER" in
  100m-64m)  BENCH_CPU=0.1 BENCH_MEM=64m   ;;
  200m-128m) BENCH_CPU=0.2 BENCH_MEM=128m  ;;
  *)
    printf 'usage: bench/run.sh <100m-64m|200m-128m> [single|batch]\n' >&2
    exit 2
    ;;
esac
# The tier is authoritative: an inherited BENCH_CPU/BENCH_MEM must not
# silently change what the run measures.
export BENCH_CPU BENCH_MEM

SCENARIO=${2:-single}
case "$SCENARIO" in
  single|batch) ;;
  *) printf 'unknown scenario: %s\n' "$SCENARIO" >&2; exit 2 ;;
esac

RATES=${RATES:-"50 100 200 400"}
DURATION=${DURATION:-30s}
VUS=${VUS:-64}
BATCH_SIZE=${BATCH_SIZE:-50}
# k6 runs inside the broker's container network by default, so this is a
# service name, not a host address. Nothing is published on the host.
BROKER_URL=${BROKER_URL:-http://rs-broker-producer:8080}
SETTLE=${SETTLE:-20}
DRAIN_WAIT=${DRAIN_WAIT:-180}
RESULT_DIR=${RESULT_DIR:-$REPO_ROOT/bench/results}

COMPOSE=(docker compose -f compose.yml -f bench/compose.bench.yml)
PROFILES=(--profile producer --profile consumer)
ALL_CONTAINERS="rs-broker-producer rs-broker-consumer rs-broker-postgres rs-broker-kafka"

# k6 runs inside the broker's container network by default: no request
# pays a host-to-VM network hop, and the benchmark publishes no host
# ports, so it cannot clash with another stack on the machine. Set
# BENCH_K6_HOST=1 to run k6 on the host instead, which measures the hop.
if [ "${BENCH_K6_HOST:-0}" = "1" ]; then
  k6_run() { k6 run "$@"; }
  K6_SCRIPT=bench/k6-publish.js
  K6_RESULTS=$RESULT_DIR
else
  # The k6 image's entrypoint is already `k6`, so the arguments start with
  # the subcommand.
  k6_run() { "${COMPOSE[@]}" --profile bench run --rm -T bench-k6 run "$@"; }
  K6_SCRIPT=/k6-publish.js
  K6_RESULTS=/results
fi

cleanup() {
  if [ "${KEEP:-0}" != "1" ]; then
    "${COMPOSE[@]}" "${PROFILES[@]}" --profile bench down -v >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

query() {
  "${COMPOSE[@]}" exec -T postgres psql -U rsbroker -d rsbroker -At -c "$1" 2>/dev/null |
    tr -d '[:space:]'
}

# cgroup CPU consumption of a container, in microseconds.
usage_usec() {
  docker exec "$1" cat /sys/fs/cgroup/cpu.stat 2>/dev/null | awk '/^usage_usec/ {print $2}'
}

# Peak memory of a container in MiB.
mem_peak_mb() {
  docker exec "$1" cat /sys/fs/cgroup/memory.peak 2>/dev/null |
    awk '/^[0-9]+/ {printf "%.1f", $1/1048576}'
}

# Snapshot of every container's CPU counter.
snapshot() {
  for c in $ALL_CONTAINERS; do
    printf '%s %s\n' "$c" "$(usage_usec "$c")"
  done
}

# Average cores used by one container between two snapshots.
cores_between() {
  local container=$1 before=$2 after=$3 wall=$4
  local start end delta
  start=$(awk -v name="$container" '$1 == name {print $2}' "$before")
  end=$(awk -v name="$container" '$1 == name {print $2}' "$after")
  delta=$((end - start))
  awk -v d="$delta" -v w="$wall" 'BEGIN { printf "%.4f", d / 1000000 / w }'
}

mkdir -p "$RESULT_DIR"
TSV="$RESULT_DIR/results-$TIER-$SCENARIO.tsv"
if [ ! -f "$TSV" ]; then
  printf 'rate\thttp_reqs\trps\treq_failed_pct\tavg_ms\tp95_ms\tp99_ms\tmax_ms\toutbox_delta\tpublished_delta\tinbox_delta\tfailed_outbox\tretrying_outbox\tproducer_cores_ingest\tconsumer_cores_ingest\tdrain_s\tproducer_cores_drain\tconsumer_cores_drain\tpostgres_cores_ingest\tkafka_cores_ingest\tproducer_mem_peak_mb\tconsumer_mem_peak_mb\n' > "$TSV"
fi

printf '== tier %s: cpu=%s cores, memory=%s ==\n' "$TIER" "$BENCH_CPU" "$BENCH_MEM"
printf '== scenario %s rates=[%s] duration=%s vus=%s ==\n' "$SCENARIO" "$RATES" "$DURATION" "$VUS"

# The broker image is tagged rs-broker:bench; both services build to that tag.
"${COMPOSE[@]}" build rs-broker-producer

"${COMPOSE[@]}" "${PROFILES[@]}" up -d postgres kafka
"${COMPOSE[@]}" "${PROFILES[@]}" up -d rs-broker-producer rs-broker-consumer

printf '== waiting for the producer to become healthy ==\n'
for _ in $(seq 1 60); do
  "${COMPOSE[@]}" --profile bench run --rm -T --entrypoint wget bench-k6 \
    -q -O /dev/null "$BROKER_URL/health" >/dev/null 2>&1 && break
  sleep 2
done
"${COMPOSE[@]}" --profile bench run --rm -T --entrypoint wget bench-k6 \
  -q -O /dev/null "$BROKER_URL/health" >/dev/null

# Warmup: a freshly started broker pays connection-pool, page-cache and
# Kafka-metadata costs on its first requests. A low-rate warmup does not
# remove them, so the warmup runs at the first measured rate and is
# discarded.
printf '== warmup ==\n'
k6_run \
  -e "SCENARIO=$SCENARIO" \
  -e "RATE=$(echo "$RATES" | awk '{print $1}')" \
  -e "DURATION=$DURATION" \
  -e "VUS=$VUS" \
  -e "BATCH_SIZE=$BATCH_SIZE" \
  -e "BROKER_URL=$BROKER_URL" \
  "$K6_SCRIPT" > "$RESULT_DIR/k6-warmup.log" 2>&1 || true

# The warmup leaves a backlog in the outbox. Drain it before measuring, or
# the publisher is still working through warmup traffic while the measured
# run starts, and the next rate inherits that backlog.
printf '== draining warmup backlog ==\n'
for _ in $(seq 1 "$DRAIN_WAIT"); do
  PENDING=$(query "select count(*) from outbox_messages where status in ('pending','publishing','retrying')")
  [ "${PENDING:-1}" = "0" ] && break
  sleep 1
done

RUN_TAG="$TIER-$SCENARIO"
WALL=$(echo "$DURATION" | sed 's/s$//')

for RATE in $RATES; do
  TAG="$RUN_TAG-r$RATE"

  printf '== settle %ss ==\n' "$SETTLE"
  sleep "$SETTLE"

  BEFORE_OUTBOX=$(query 'select count(*) from outbox_messages')
  BEFORE_PUBLISHED=$(query "select count(*) from outbox_messages where status = 'published'")
  BEFORE_FAILED=$(query "select count(*) from outbox_messages where status in ('failed','dlq')")
  BEFORE_INBOX=$(query 'select count(*) from inbox_messages')

  SNAP_BEFORE=$(mktemp)
  snapshot > "$SNAP_BEFORE"

  printf '== k6 run rate=%s ==\n' "$RATE"
  set +e
  k6_run \
    --summary-export "$K6_RESULTS/k6-$TAG-summary.json" \
    -e "SCENARIO=$SCENARIO" \
    -e "RATE=$RATE" \
    -e "DURATION=$DURATION" \
    -e "VUS=$VUS" \
    -e "BATCH_SIZE=$BATCH_SIZE" \
    -e "BROKER_URL=$BROKER_URL" \
    "$K6_SCRIPT" > "$RESULT_DIR/k6-$TAG.log" 2>&1
  STATUS=$?
  set -e
  tail -6 "$RESULT_DIR/k6-$TAG.log"

  SNAP_INGEST=$(mktemp)
  snapshot > "$SNAP_INGEST"

  # Let the outbox drain and the consumer catch up before counting.
  # Both conditions must hold: no unpublished outbox rows left, and the
  # inbox count caught up with what was published. A publisher failure
  # parks a row in `failed`/`dlq`, so those count as settled too.
  printf '== draining (max %ss) ==\n' "$DRAIN_WAIT"
  DRAIN_START=$(date +%s)
  for _ in $(seq 1 "$DRAIN_WAIT"); do
    PENDING=$(query "select count(*) from outbox_messages where status in ('pending','publishing','retrying')")
    PUBLISHED_NOW=$(query "select count(*) from outbox_messages where status = 'published'")
    INBOX_NOW=$(query 'select count(*) from inbox_messages')
    if [ "${PENDING:-1}" = "0" ] && [ "${INBOX_NOW:-0}" -ge "$PUBLISHED_NOW" ]; then
      break
    fi
    sleep 1
  done
  DRAIN_END=$(date +%s)
  DRAIN_S=$((DRAIN_END - DRAIN_START))

  SNAP_AFTER=$(mktemp)
  snapshot > "$SNAP_AFTER"

  AFTER_OUTBOX=$(query 'select count(*) from outbox_messages')
  AFTER_PUBLISHED=$(query "select count(*) from outbox_messages where status = 'published'")
  AFTER_INBOX=$(query 'select count(*) from inbox_messages')
  FAILED_OUTBOX=$(query "select count(*) from outbox_messages where status in ('failed','dlq')")
  FAILED_DELTA=$((FAILED_OUTBOX - BEFORE_FAILED))
  RETRYING_OUTBOX=$(query "select count(*) from outbox_messages where status = 'retrying'")

  SUMMARY="$RESULT_DIR/k6-$TAG-summary.json"
  REQS=$(jq -r '.metrics.http_reqs.count // 0' "$SUMMARY")
  RPS=$(jq -r '.metrics.http_reqs.rate // 0' "$SUMMARY")
  FAILED_PCT=$(jq -r '(.metrics.http_req_failed.value // 0) * 100' "$SUMMARY")
  AVG=$(jq -r '.metrics["http_req_duration{expected_response:true}"].avg // 0' "$SUMMARY")
  P95=$(jq -r '.metrics["http_req_duration{expected_response:true}"]["p(95)"] // 0' "$SUMMARY")
  P99=$(jq -r '.metrics["http_req_duration{expected_response:true}"]["p(99)"] // 0' "$SUMMARY")
  MAX=$(jq -r '.metrics["http_req_duration{expected_response:true}"].max // 0' "$SUMMARY")

  # Ingest window: the k6 phase, where HTTP requests hit the producer.
  PROD_INGEST=$(cores_between rs-broker-producer "$SNAP_BEFORE" "$SNAP_INGEST" "$WALL")
  CONS_INGEST=$(cores_between rs-broker-consumer "$SNAP_BEFORE" "$SNAP_INGEST" "$WALL")
  PG_INGEST=$(cores_between rs-broker-postgres "$SNAP_BEFORE" "$SNAP_INGEST" "$WALL")
  KAFKA_INGEST=$(cores_between rs-broker-kafka "$SNAP_BEFORE" "$SNAP_INGEST" "$WALL")

  # Drain window: the publisher moves outbox rows to Kafka and the consumer
  # writes inbox rows. This is where the broker actually does the work.
  if [ "$DRAIN_S" -gt 0 ]; then
    PROD_DRAIN=$(cores_between rs-broker-producer "$SNAP_INGEST" "$SNAP_AFTER" "$DRAIN_S")
    CONS_DRAIN=$(cores_between rs-broker-consumer "$SNAP_INGEST" "$SNAP_AFTER" "$DRAIN_S")
  else
    PROD_DRAIN=0
    CONS_DRAIN=0
  fi

  rm -f "$SNAP_BEFORE" "$SNAP_INGEST" "$SNAP_AFTER"

  OUTBOX_DELTA=$((AFTER_OUTBOX - BEFORE_OUTBOX))
  PUBLISHED_DELTA=$((AFTER_PUBLISHED - BEFORE_PUBLISHED))
  INBOX_DELTA=$((AFTER_INBOX - BEFORE_INBOX))
  PROD_MEM=$(mem_peak_mb rs-broker-producer)
  CONS_MEM=$(mem_peak_mb rs-broker-consumer)

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$RATE" "$REQS" "$RPS" "$FAILED_PCT" "$AVG" "$P95" "$P99" "$MAX" \
    "$OUTBOX_DELTA" "$PUBLISHED_DELTA" "$INBOX_DELTA" \
    "$FAILED_DELTA" "$RETRYING_OUTBOX" \
    "$PROD_INGEST" "$CONS_INGEST" "$DRAIN_S" \
    "$PROD_DRAIN" "$CONS_DRAIN" "$PG_INGEST" "$KAFKA_INGEST" \
    "$PROD_MEM" "$CONS_MEM" >> "$TSV"

  printf '== rate=%s status=%s reqs=%s rps=%s outbox=%s published=%s inbox=%s failed=%s ingest_cores(p/c)=%s/%s drain_s=%s drain_cores(p/c)=%s/%s ==\n' \
    "$RATE" "$STATUS" "$REQS" "$RPS" "$OUTBOX_DELTA" "$PUBLISHED_DELTA" "$INBOX_DELTA" "$FAILED_DELTA" \
    "$PROD_INGEST" "$CONS_INGEST" "$DRAIN_S" "$PROD_DRAIN" "$CONS_DRAIN"
done

printf '== results ==\n'
cat "$TSV"
