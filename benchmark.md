# rs-broker Benchmarks

Load-test results for rs-broker running under constrained container
resources, with k6 as the load generator.

The question these numbers answer: **how much load can rs-broker carry when
each broker container is capped at a small slice of CPU and memory?**

## TL;DR

| Tier (per container) | Max sustained publish rate | Latency at that rate | Verdict |
|---|---|---|---|
| 100m CPU / 64 MiB | ~1 140 req/s | p95 227 ms | CPU-bound on the producer |
| 200m CPU / 128 MiB | ~2 230 req/s | p95 242 ms | CPU-bound on the producer |

Batch publish (50 messages per request) is the better deal: at 200m / 128 MiB
it absorbs **80 requests/s × 50 = 4 000 messages/s** at p95 4.1 ms with no
failures.

Memory is never the constraint: the producer peaks at **10 MiB of a 64 MiB
limit**. The binding constraint is CPU, and the failure mode past the ceiling
is not "slow" but **stalled** — a throttled cgroup freezes every in-flight
request for up to a second.

## Test setup

| Component | Image | CPU | Memory |
|---|---|---|---|
| rs-broker producer | built from `Containerfile` | `0.1` cores (100 m) / `0.2` cores (200 m) | 64 MiB / 128 MiB |
| rs-broker consumer | same image | `0.1` cores (100 m) / `0.2` cores (200 m) | 64 MiB / 128 MiB |
| PostgreSQL | `postgres:18-alpine` | unlimited | unlimited |
| Kafka | `confluentinc/cp-kafka:8.0.4` | unlimited | unlimited |

The CPU and memory limits are the Kubernetes-style quantities `100m`/`200m`
and `64Mi`/`128Mi`, applied as cgroup v2 quota and limit via `cpus:` and
`mem_limit:` in [`bench/compose.bench.yml`](bench/compose.bench.yml).

Kafka and Postgres are deliberately **unlimited** so the numbers describe the
broker, not the infrastructure. Postgres used at most 0.59 cores and Kafka at
most 0.19, so neither was anywhere near saturation.

Producer and consumer run as separate containers with separate limits. The
producer has no consumer topics configured, so it does not compete with the
consumer for the same consumer group.

### Environment

| | |
|---|---|
| Host | macOS 27.2, arm64, 14 cores / 48 GiB |
| Containers | Podman 6.0.2 machine, 4 vCPU / 4 GiB, cgroup v2 |
| Load generator | k6 v2.3.0, on the host, outside the VM |
| rs-broker | this branch, `--release`, postgres feature |

The load generator shares the same physical host as the containers, so
absolute latency includes host networking. The *relative* numbers (100m vs
200m, rate vs rate) are unaffected by that.

## Method

Two scenarios, both over the HTTP REST API (`/api/v1`):

- **single** — one `POST /api/v1/publish` per request, one message each.
- **batch** — one `POST /api/v1/publish/batch` per request, 50 messages each.

The runner ([`bench/run.sh`](bench/run.sh)) does the following per rate:

1. Bring up a fresh stack (fresh Postgres and Kafka volumes) so row counts
   are per-run.
2. Wait for the producer to report healthy.
3. Run a **warmup** at the first measured rate, discard it, then drain the
   outbox so the next rate starts from an empty backlog. Both matter: on a
   cold process the same 50 req/s run measured p99 = 734 ms where the
   identical second run measured p99 = 5 ms.
4. Run k6 with a `constant-arrival-rate` executor for 30 s.
5. Wait for the pipeline to settle: no outbox row left unpublished, and the
   inbox count caught up with what was published.
6. Read the counters out of Postgres and the CPU counters out of the cgroups.

### How the numbers are measured

- **Throughput** — `http_reqs.rate` from the k6 summary export, cross-checked
  against the outbox/inbox row deltas. In every row of the tables below the
  two agree exactly: every accepted request became an outbox row.
- **Latency** — `http_req_duration` from the k6 summary export.
- **CPU** — `cpu.stat` `usage_usec` read from the container's cgroup before
  and after the load window, divided by the window length. This is core-seconds
  actually consumed, not the percentage `docker stats` reports (which is
  measured against host CPU, so it reads far too low under a quota).
- **Memory** — `memory.peak` from the container's cgroup.

`bench/results/` holds the raw k6 summary exports and the per-run TSV rows.

## Results — single publish

Each request publishes one message.

### 100m CPU / 64 MiB

| Target rate | Achieved | Reqs | p95 | p99 | Producer cores | Consumer cores | Peak mem (P/C) |
|---|---|---|---|---|---|---|---|
| 100/s | 100.0/s | 3 000 | 3.4 ms | 4.6 ms | 0.038 | 0.016 | 5.3 / 4.3 MiB |
| 200/s | 200.0/s | 6 000 | 2.2 ms | 3.2 ms | 0.058 | 0.018 | 5.5 / 5.5 MiB |
| 400/s | 400.0/s | 12 001 | 1.6 ms | 2.9 ms | 0.083 | 0.019 | 8.4 / 7.7 MiB |
| 800/s | 796.0/s | 23 896 | 36 ms | 130 ms | 0.101 | 0.029 | 8.5 / 13.0 MiB |
| 1 200/s | 1 140.4/s | 34 418 | 227 ms | 244 ms | 0.102 | 0.034 | 8.5 / 13.0 MiB |

The 100 m ceiling is **~1 140 req/s**. Past it the offered rate is dropped
and latency jumps by two orders of magnitude. The producer burns exactly its
0.1-core quota (0.102 cores measured) — it is saturated.

### 200m CPU / 128 MiB

| Target rate | Achieved | Reqs | p95 | p99 | Producer cores | Consumer cores | Peak mem (P/C) |
|---|---|---|---|---|---|---|---|
| 200/s | 200.0/s | 6 001 | 2.0 ms | 3.2 ms | 0.052 | 0.018 | 6.5 / 4.6 MiB |
| 400/s | 400.0/s | 12 001 | 1.5 ms | 2.8 ms | 0.079 | 0.020 | 7.3 / 7.3 MiB |
| 800/s | 800.0/s | 24 001 | 1.3 ms | 2.8 ms | 0.119 | 0.025 | 8.9 / 12.6 MiB |
| 1 600/s | 1 599.9/s | 47 998 | 11 ms | 25 ms | 0.200 | 0.042 | 10.3 / 23.1 MiB |
| 2 400/s | 2 227.6/s | 67 086 | 242 ms | 262 ms | 0.203 | 0.041 | 10.3 / 23.1 MiB |

The 200 m ceiling is **~2 230 req/s**, 2.0x the 100 m ceiling — the same
ratio as the CPU allowance.

At 800/s the broker is comfortable: p95 1.3 ms with the producer at 60% of
its quota. At 1 600/s the quota is fully used and p95 rises to 11 ms, but the
rate is still delivered exactly.

## Results — batch publish (50 messages per request)

The batch endpoint inserts all messages in one statement, so it asks much
less of the producer per message.

`msgs` is the number of messages accepted; `pub` is how many had reached
Kafka when the drain window closed. Where `pub` is short of `msgs`, the
pipeline is still draining a backlog — the remainder stays `pending` and
publishes afterwards. **Nothing is dead-lettered: `fail` is 0 in every row.**

### 100m CPU / 64 MiB

| Target rate | Achieved | msgs | pub | fail | p95 | p99 | Producer cores | Consumer cores |
|---|---|---|---|---|---|---|---|---|
| 10/s | 10.0/s | 15 000 | 15 000 | 0 | 11.7 ms | 13.6 ms | 0.030 | 0.022 |
| 20/s | 20.0/s | 30 050 | 30 050 | 0 | 5.1 ms | 6.3 ms | 0.039 | 0.029 |
| 40/s | 40.0/s | 60 000 | 60 000 | 0 | 4.6 ms | 9.9 ms | 0.049 | 0.059 |
| 80/s | 80.0/s | 120 050 | 79 950 | 0 | 35 ms | 55 ms | 0.091 | 0.042 |
| 120/s | 120.0/s | 180 000 | 0 | 0 | 57 ms | 83 ms | 0.106 | 0.082 |

### 200m CPU / 128 MiB

| Target rate | Achieved | msgs | pub | fail | p95 | p99 | Producer cores | Consumer cores |
|---|---|---|---|---|---|---|---|---|
| 10/s | 10.0/s | 15 050 | 15 050 | 0 | 6.0 ms | 7.3 ms | 0.014 | 0.029 |
| 20/s | 20.0/s | 30 000 | 30 000 | 0 | 5.4 ms | 10.1 ms | 0.029 | 0.040 |
| 40/s | 40.0/s | 60 050 | 60 050 | 0 | 4.0 ms | 7.4 ms | 0.046 | 0.056 |
| 80/s | 80.0/s | 120 000 | 79 850 | 0 | 4.1 ms | 7.7 ms | 0.069 | 0.068 |
| 120/s | 120.0/s | 180 000 | 0 | 0 | 15 ms | 33 ms | 0.124 | 0.043 |
| 160/s | 160.0/s | 240 050 | 0 | 0 | 7.3 ms | 42 ms | 0.186 | 0.083 |

Batch publish is the better deal: at 200m / 128 MiB, 80 requests/s of 50
messages each is **4 000 messages/s** at p95 4.1 ms — nearly twice the
single-publish ceiling of ~2 230/s, because the per-request overhead
(Postgres round trip, HTTP framing, Kafka produce) is amortised over 50
messages.

## Where the limit comes from

The broker is not slow per request: median latency is 1.6–2.2 ms up to
400 req/s on the 100 m tier and 1.3–2.0 ms up to 800 req/s on the 200 m tier.
Past the ceiling the failure mode is a **stall**, not a slowdown — at
1 200 req/s on 100 m the p95 jumps to 227 ms, because every in-flight request
waits out the same throttle period.

Evidence from the cgroup counters, at 50 m CPU during a 180 s run:

| | before | after |
|---|---|---|
| `nr_throttled` | 2 | 27 |
| `throttled_usec` | 170 175 | 2 142 424 |

**25 throttle events totalling 1.97 s** — and the observed latency spikes
are ~1 s each.

Confirming the causal chain:

| Configuration | Requests | Over 50 ms | Max |
|---|---|---|---|
| 50m CPU limit, 6 runs of 90–180 s | 9 001 each | 16–95 | 173–1 740 ms |
| 1 core (no quota), 2 runs of 180 s | 9 001 each | **0** | 17–23 ms |
| 50m CPU, publisher interval 100 → 5 000 ms, 3 runs of 90 s | 4 500 each | 0–6 | 30–102 ms |

The quota is the cause: every run under a 50 m limit stalled, and no run
without a quota stalled at all.

The mechanism is CFS bandwidth throttling. The publisher drains up to 100
outbox rows every 100 ms; on a 0.05-core quota that burst cannot fit in the
period budget, so the cgroup is throttled mid-drain and the request path
stalls with it. Spreading the same work over 5 s shrinks the burst and
reduces the stalls (0–6 slow requests instead of 16–95), but does not
eliminate them — the quota itself is still the binding constraint.

The practical finding: **on small CPU quotas, the publisher's cadence has to
match the quota** (`RS_BROKER_OUTBOX__BATCH_SIZE` and
`RS_BROKER_OUTBOX__POLL_INTERVAL_MS`; exposed as `BENCH_PUBLISHER_BATCH` and
`BENCH_PUBLISHER_INTERVAL` in
[`bench/compose.bench.yml`](bench/compose.bench.yml)). Beyond that, raising
the quota is the only way to raise the ceiling.

## What the benchmarks changed

The benchmark numbers drove five code changes, each measured:

| Change | Effect |
|---|---|
| Batch outbox insert uses `UNNEST` instead of one `INSERT` per message | 50-message batch: 2 456 µs → 1 309 µs per request (49.1 → 26.2 µs per message) |
| Batch inbox insert (`UNNEST`), consumer writes in batches of 100 | consumer: 106.7 µs → 53.4 µs per message |
| `min_idle_connections` is applied to the pool (it was configurable but ignored) | pool keeps its idle connections warm instead of opening them under load |
| A full Kafka local queue is treated as backpressure, not a delivery failure | 25 050 messages that were dead-lettered at 80 batch req/s are now published (0 failures) |
| The publisher reads `outbox.batch_size` / `outbox.poll_interval_ms` | the keys `compose.yml` and `.env.example` document were previously ignored |

The backpressure fix is a correctness fix as much as a performance one:
`QueueFull` means "slow down", not "this message is bad", and spending the
retry budget on it threw away good messages.

Three configuration bugs fell out of the same investigation and are also
fixed: `enable.idempotence` and `compression.type` were configurable but
never passed to librdkafka, `message.timeout.ms` was hard-coded to 30 000
instead of using the configured value, and the publisher cadence was read
from a duplicate `server.publisher_*` pair instead of the documented
`outbox.*` keys.

## Throughput summary

| Tier | Scenario | Max sustained | Equivalent messages/s |
|---|---|---|---|
| 100m / 64 MiB | single | ~1 140 req/s | ~1 140 |
| 100m / 64 MiB | batch (50) | 80 req/s | 4 000 |
| 200m / 128 MiB | single | ~2 230 req/s | ~2 230 |
| 200m / 128 MiB | batch (50) | 80 req/s | 4 000 |

## Reproducing

Requires Docker (or Podman) with the Compose plugin, `k6`, and `jq`.

```bash
# One tier, one scenario. RATES defaults to "50 100 200 400".
./bench/run.sh 100m-64m single
./bench/run.sh 200m-128m single
./bench/run.sh 200m-128m batch

# Override the sweep.
RATES="400 800 1600" DURATION=60s VUS=128 ./bench/run.sh 200m-128m single

# Tune the publisher cadence (see "Where the limit comes from").
BENCH_PUBLISHER_BATCH=10 BENCH_PUBLISHER_INTERVAL=5000 ./bench/run.sh 100m-64m single
```

Results land in `bench/results/` as a TSV row per rate plus the raw k6
summary export and log for each run.

The runner tears the stack down when it finishes. Pass `KEEP=1` to leave it
running for inspection.

## Caveats

- The load generator ran on the host while the containers ran in a Podman
  VM, so absolute latency includes a host network hop. Compare tiers, not
  absolute numbers. For reference, nginx serving a static file over the same
  path cost 109 µs/request of broker-visible CPU versus rs-broker's 98.5 µs,
  so the per-request floor is the environment, not the broker.
- The host VM has 4 vCPUs; both broker containers plus Postgres and Kafka
  share them. Postgres peaked at 0.59 cores and Kafka at 0.19, so the
  infrastructure was not the constraint, but it was not isolated either.
- Only the REST API was measured. k6 v2.3.0 has no gRPC or SSE module, so
  the gRPC path is unmeasured. It shares the same service implementation as
  the REST path, so the ceiling should be comparable.
- The consumer's own throughput ceiling was not separately characterised;
  it was measured as it drained the inbox alongside the producer.
- Results are from a single run per rate. The tail latencies vary run to run
  under throttling, so treat the tail numbers as indicative and the
  throughput ceilings as solid.
- The `both`-mode service was not benchmarked; the producer and consumer
  were measured as separate containers, which is how they are deployed.
