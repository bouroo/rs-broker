// k6 load profile for the rs-broker REST API.
//
// One scenario per run, selected with SCENARIO:
//   single  constant-arrival-rate POST /api/v1/publish
//   batch   constant-arrival-rate POST /api/v1/publish/batch
//
// Knobs (env):
//   BROKER_URL   producer base URL        (default http://localhost:8080)
//   RATE         requests per second       (default 20)
//   DURATION     test duration             (default 60s)
//   VUS          pre-allocated VUs         (default 32)
//   BATCH_SIZE   messages per batch call   (default 50)
//   TOPIC        target Kafka topic        (default orders)

import http from 'k6/http';
import { check, fail } from 'k6';

const BROKER_URL = __ENV.BROKER_URL || 'http://localhost:8080';
const RATE = Number(__ENV.RATE || 20);
const DURATION = __ENV.DURATION || '60s';
const VUS = Number(__ENV.VUS || 32);
const BATCH_SIZE = Number(__ENV.BATCH_SIZE || 50);
const TOPIC = __ENV.TOPIC || 'orders';
const SCENARIO = __ENV.SCENARIO || 'single';

const JSON_HEADERS = { 'Content-Type': 'application/json' };

export const options = {
  scenarios: {
    [SCENARIO]: {
      executor: 'constant-arrival-rate',
      rate: RATE,
      timeUnit: '1s',
      duration: DURATION,
      preAllocatedVUs: VUS,
      maxVUs: VUS * 4,
      exec: SCENARIO,
    },
  },
  thresholds: {
    http_req_failed: ['rate<0.01'],
    http_req_duration: ['p(95)<2000'],
  },
  // The batch scenario asserts on the response body, so bodies are kept.
  discardResponseBodies: false,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

function message(i) {
  return {
    aggregate_type: 'Order',
    aggregate_id: `bench-${__VU}-${i}`,
    event_type: 'OrderCreated',
    payload: { amount: 100 + i, currency: 'USD' },
    topic: TOPIC,
  };
}

export function single() {
  const res = http.post(`${BROKER_URL}/api/v1/publish`, JSON.stringify(message(1)), {
    headers: JSON_HEADERS,
    tags: { name: 'publish' },
  });
  const ok = check(res, { 'publish accepted': (r) => r.status === 200 });
  if (!ok) {
    fail(`publish failed: status=${res.status} body=${res.body}`);
  }
}

export function batch() {
  const messages = [];
  for (let i = 0; i < BATCH_SIZE; i++) {
    messages.push(message(i));
  }
  const res = http.post(`${BROKER_URL}/api/v1/publish/batch`, JSON.stringify({ messages }), {
    headers: JSON_HEADERS,
    tags: { name: 'publish_batch' },
  });
  const ok = check(res, {
    'batch accepted': (r) => r.status === 200,
    'batch fully stored': (r) => {
      if (r.status !== 200) return false;
      try {
        return JSON.parse(r.body).failure_count === 0;
      } catch (e) {
        return false;
      }
    },
  });
  if (!ok) {
    fail(`batch publish failed: status=${res.status} body=${res.body}`);
  }
}
