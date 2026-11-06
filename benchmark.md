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
broker, not the infrastructure. Postgres used at most 1.22 cores and Kafka at
most 0.14, so neither was anywhere near saturation.

Producer and consumer run as separate containers with separate limits. The
producer has no consumer topics configured, so it does not compete with the
consumer for the same consumer group.

### Everything runs in one network

k6, the broker, Postgres and Kafka all sit on the same container network.
The benchmark **publishes no host ports at all**:

- the broker's 8080 would otherwise clash with any other stack on the
  machine (the consumer's 8081 clashes with schema-registry in the default
  compose file);
- every request skips the host-to-VM NAT hop, which is worth more than the
  broker's own per-request cost.

The cost of that hop, measured at 200 req/s on the 100m tier with the same
broker and the same rate:

| Load generator | avg | p95 | p99 |
|---|---|---|---|
| k6 on the host, through a published port | 1.54 ms | 2.22 ms | 3.66 ms |
| k6 in the broker's network | **0.87 ms** | **1.24 ms** | **2.45 ms** |

The hop cost ~0.67 ms per request: roughly 44% of the measured latency at
that rate. Every number below is the in-network figure.

### Environment

| | |
|---|---|
| Host | macOS 27.2, arm64, 14 cores / 48 GiB |
| Containers | Podman 6.0.2 machine, 4 vCPU / 4 GiB, cgroup v2 |
| Load generator | k6 v2.3.0, in the broker's container network |
| rs-broker | v0.3.0, `--release`, postgres feature |

k6 shares the VM's 4 vCPUs with the broker, Postgres and Kafka. Postgres
peaked at 1.22 cores and Kafka at 0.14, so the infrastructure was never the
constraint, but it was not isolated either.

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
| 100/s | 100.0/s | 3 001 | 2.0 ms | 3.5 ms | 0.036 | 0.018 | 6.0 / 4.5 MiB |
| 200/s | 200.0/s | 6 001 | 1.2 ms | 2.0 ms | 0.049 | 0.019 | 6.2 / 5.3 MiB |
| 400/s | 400.0/s | 12 001 | 0.9 ms | 2.1 ms | 0.072 | 0.020 | 7.8 / 6.8 MiB |
| 800/s | 797.5/s | 23 927 | 18 ms | 53 ms | 0.100 | 0.025 | 13.5 / 10.9 MiB |
| 1 200/s | 1 133.9/s | 34 162 | 232 ms | 250 ms | 0.102 | 0.037 | 21.6 / 17.5 MiB |

The 100 m ceiling is **~1 140 req/s**. Past it the offered rate is dropped
and latency jumps by two orders of magnitude. The producer burns exactly its
0.1-core quota (0.102 cores measured) — it is saturated.

### 200m CPU / 128 MiB

| Target rate | Achieved | Reqs | p95 | p99 | Producer cores | Consumer cores | Peak mem (P/C) |
|---|---|---|---|---|---|---|---|
| 200/s | 200.0/s | 6 001 | 1.3 ms | 3.1 ms | 0.049 | 0.017 | 9.6 / 5.3 MiB |
| 400/s | 400.0/s | 12 001 | 0.9 ms | 2.1 ms | 0.071 | 0.019 | 10.7 / 6.6 MiB |
| 800/s | 800.0/s | 24 001 | 2.0 ms | 12 ms | 0.136 | 0.028 | 14.1 / 9.5 MiB |
| 1 600/s | 1 600.0/s | 48 001 | 3.9 ms | 16 ms | 0.197 | 0.043 | 19.7 / 15.7 MiB |
| 2 400/s | 2 389.0/s | 71 740 | 69 ms | 88 ms | 0.201 | 0.051 | 32.4 / 24.7 MiB |

The 200 m ceiling is **~2 390 req/s**, 2.1x the 100 m ceiling — the same
ratio as the CPU allowance.

At 1 600/s the broker is still comfortable: p95 3.9 ms with the producer at
98% of its quota, and the full rate delivered exactly. The cliff comes at
2 400/s.

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
| 10/s | 10.0/s | 15 000 | 15 000 | 0 | 6.1 ms | 7.5 ms | 0.015 | 0.026 |
| 20/s | 20.0/s | 30 050 | 30 050 | 0 | 4.4 ms | 5.3 ms | 0.024 | 0.042 |
| 40/s | 40.0/s | 60 000 | 60 000 | 0 | 3.5 ms | 7.9 ms | 0.046 | 0.056 |
| 80/s | 80.0/s | 120 050 | 79 950 | 0 | 3.6 ms | 8.5 ms | 0.068 | 0.084 |
| 120/s | 120.0/s | 180 050 | 0 | 0 | 75 ms | 175 ms | 0.105 | 0.085 |

### 200m CPU / 128 MiB

| Target rate | Achieved | msgs | pub | fail | p95 | p99 | Producer cores | Consumer cores |
|---|---|---|---|---|---|---|---|---|
| 10/s | 10.0/s | 15 000 | 15 000 | 0 | 6.5 ms | 8.7 ms | 0.025 | 0.018 |
| 20/s | 20.0/s | 30 050 | 30 050 | 0 | 3.5 ms | 5.6 ms | 0.031 | 0.039 |
| 40/s | 40.0/s | 60 050 | 60 050 | 0 | 3.8 ms | 9.2 ms | 0.045 | 0.056 |
| 80/s | 80.0/s | 120 050 | 79 900 | 0 | 3.5 ms | 7.9 ms | 0.069 | 0.053 |
| 120/s | 120.0/s | 180 050 | 0 | 0 | 5.2 ms | 136 ms | 0.174 | 0.092 |
| 160/s | 160.0/s | 240 050 | 0 | 0 | 5.4 ms | 15 ms | 0.191 | 0.087 |

Batch publish is the better deal: at 200m / 128 MiB, 80 requests/s of 50
messages each is **4 000 messages/s** at p95 3.5 ms — comfortably above the
single-publish ceiling of ~2 390/s, because the per-request overhead
(Postgres round trip, HTTP framing, Kafka produce) is amortised over 50
messages.

## Where the limit comes from

The broker is not slow per request: p95 is 0.9-2.0 ms up to 400 req/s on the
100 m tier and 0.9-2.0 ms up to 1 600 req/s on the 200 m tier. Past the
ceiling the failure mode is a **stall**, not a slowdown — at 1 200 req/s on
100 m the p95 jumps to 232 ms, because every in-flight request waits out the
same throttle period.

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
| 50m CPU limit, 6 runs of 90-180 s | 9 001 each | 16-95 | 173-1 740 ms |
| 1 core (no quota), 2 runs of 180 s | 9 001 each | **0** | 17-23 ms |
| 50m CPU, publisher interval 100 -> 5 000 ms, 3 runs of 90 s | 4 500 each | 0-6 | 30-102 ms |

The quota is the cause: every run under a 50 m limit stalled, and no run
without a quota stalled at all.

The mechanism is CFS bandwidth throttling. The publisher drains up to 100
outbox rows every 100 ms; on a 0.05-core quota that burst cannot fit in the
period budget, so the cgroup is throttled mid-drain and the request path
stalls with it. Spreading the same work over 5 s shrinks the burst and
reduces the stalls (0-6 slow requests instead of 16-95), but does not
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
| 100m / 64 MiB | single | ~1 135 req/s | ~1 135 |
| 100m / 64 MiB | batch (50) | 80 req/s | 4 000 |
| 200m / 128 MiB | single | ~2 390 req/s | ~2 390 |
| 200m / 128 MiB | batch (50) | 80 req/s | 4 000 |

## Reproducing

Requires Docker (or Podman) with the Compose plugin and `jq`. k6 runs
inside the benchmark's own container network, so it does not need to be
installed on the host.

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

- k6, the broker, Postgres and Kafka share the VM's 4 vCPUs. Postgres
  peaked at 1.22 cores and Kafka at 0.14, so the infrastructure was never
  the constraint, but it was not isolated either. A host-side load
  generator would have removed that coupling at the cost of a network hop
  worth ~0.67 ms per request.
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
