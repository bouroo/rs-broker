# HTTP REST + SSE API

rs-broker exposes its full broker interface over two transports that share one
state and one implementation:

- **gRPC** (tonic) on `server.grpc_port` (default `50051`) — see
  [grpc-proto.md](grpc-proto.md)
- **HTTP REST + SSE** under `/api/v1` on `server.http_port` (default `8080`,
  the same port as `/health` and `/metrics`)

Every REST endpoint maps 1:1 to a gRPC RPC and goes through the same
transport-free service methods, so validation, persistence, and error text are
identical across transports. The only structural difference: the gRPC
bidirectional `StreamPublish` is served by the batch endpoint, and the
server-streaming `SubscribeEvents` becomes an SSE stream.

## Endpoints

| Method | Path | gRPC RPC |
|---|---|---|
| POST | `/api/v1/publish` | `Publish` |
| POST | `/api/v1/publish/batch` | `PublishBatch` / `StreamPublish` |
| GET | `/api/v1/messages/{id}` | `GetMessageStatus` |
| DELETE | `/api/v1/messages/{id}` | `CancelMessage` |
| POST | `/api/v1/subscribers` | `RegisterSubscriber` |
| GET | `/api/v1/subscribers` | `ListSubscribers` |
| PATCH | `/api/v1/subscribers/{id}` | `UpdateSubscriber` |
| DELETE | `/api/v1/subscribers/{id}` | `UnregisterSubscriber` |
| GET | `/api/v1/events/stream` | `SubscribeEvents` (SSE) |
| GET | `/api/v1/health` | `GetHealth` |
| POST | `/api/v1/dlq/reprocess` | `ReprocessDlq` |
| GET | `/api/v1/dlq` | `ListDlqMessages` |

### Field conventions

- Field names match the proto messages (snake_case). Optional proto scalars
  default exactly as proto3 defaults do (e.g. an omitted `topic` is accepted
  as an empty string).
- Enums are lowercase strings (`"pending"`, `"published"`, `"dlq"`, ...).
- `payload` follows the broker's JSON-payload contract:
  - Requests: pass `payload` (any JSON value) or `payload_base64` (raw bytes)
    — never both.
  - Responses: `payload` is the JSON value when the bytes parse as JSON,
    otherwise `payload_base64`.

### Errors

Errors use the gRPC status mapping with a JSON body:

```json
{
  "error": {
    "code": "invalid_argument",
    "message": "Invalid payload JSON: ..."
  }
}
```

| gRPC code | HTTP status |
|---|---|
| `invalid_argument`, `out_of_range` | 400 |
| `not_found` | 404 |
| `already_exists`, `failed_precondition` | 409 |
| `unauthenticated` | 401 |
| `permission_denied` | 403 |
| `unimplemented` | 501 |
| `unavailable` | 503 |
| `deadline_exceeded` | 504 |
| anything else | 500 |

## Examples

Publish and poll status:

```bash
curl -s localhost:8080/api/v1/publish \
  -H 'content-type: application/json' \
  -d '{
        "aggregate_type": "Order",
        "aggregate_id": "order-1",
        "event_type": "OrderCreated",
        "payload": {"amount": 100},
        "topic": "orders"
      }'
# {"message_id":"0198...","status":"pending","duplicate":false,"accepted_at":1756...}

curl -s localhost:8080/api/v1/messages/0198...
```

Batch publish:

```bash
curl -s localhost:8080/api/v1/publish/batch \
  -H 'content-type: application/json' \
  -d '{"messages": [{"topic": "orders", "payload": {"n": 1}}]}'
```

Subscriber CRUD:

```bash
curl -s localhost:8080/api/v1/subscribers -H 'content-type: application/json' \
  -d '{"service_name": "billing", "grpc_endpoint": "http://billing:9000",
       "topic_patterns": ["orders.*"]}'
curl -s "localhost:8080/api/v1/subscribers?active_only=true"
curl -X PATCH localhost:8080/api/v1/subscribers/<id> -H 'content-type: application/json' \
  -d '{"active": false}'
curl -X DELETE localhost:8080/api/v1/subscribers/<id>
```

DLQ admin:

```bash
curl -s "localhost:8080/api/v1/dlq?limit=20"
curl -s localhost:8080/api/v1/dlq/reprocess -H 'content-type: application/json' -d '{"all": true}'
```

## SSE event stream

`GET /api/v1/events/stream` streams the same `DeliverEvent` broadcast as the
gRPC `SubscribeEvents` server-stream.

Query parameters:

| Param | Required | Meaning |
|---|---|---|
| `subscriber_id` | yes | Non-empty consumer identifier (validated like gRPC) |
| `patterns` | no | Comma-separated topic patterns (MQTT-style `*` = one segment, `#` = zero or more); empty matches nothing |
| `position` | no | Accepted but ignored — the stream is a live broadcast from subscription time (same as gRPC) |

Event protocol:

- `event: deliver` with `data: <DeliverEvent JSON>` for each matching event
- `event: lagged` with `data: {"missed": N}` when the consumer falls behind
  the broadcast buffer (capacity 1024) — the gRPC path logs and drops instead
- keep-alive comment pings every 15 s

```bash
curl -N "localhost:8080/api/v1/events/stream?subscriber_id=demo&patterns=orders.*"

# event: deliver
# data: {"message_id":"0198...","topic":"orders.created","partition":0,
#        "offset":1,"key":"k","payload":{"amount":100},"headers":[],
#        "timestamp":1756...,"event_type":"OrderCreated"}
```

## Scope notes

- Subscriber **push delivery** (the broker calling a subscriber's callback)
  remains gRPC-only; HTTP clients consume via SSE. An HTTP callback notifier
  adapter can be added behind the same `SubscriberNotifier` port later.
- Builds with database features disabled compile the routes but respond
  `501` (`unimplemented`), matching the stub gRPC service.
