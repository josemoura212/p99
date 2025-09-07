import http from "k6/http";
import { check, sleep } from "k6";
import { uuidv4 } from "https://jslib.k6.io/k6-utils/1.4.0/index.js";

export let options = {
  stages: [
    { duration: "30s", target: 10 }, // Ramp up to 10 users over 30s
    { duration: "1m", target: 10 }, // Stay at 10 users for 1m
    { duration: "30s", target: 20 }, // Ramp up to 20 users over 30s
    { duration: "1m", target: 20 }, // Stay at 20 users for 1m
    { duration: "30s", target: 0 }, // Ramp down to 0 users
  ],
  thresholds: {
    http_req_duration: ["p(95)<1000"], // 95% of requests should be below 1000ms
    http_req_failed: ["rate<0.1"], // Error rate should be below 10%
  },
};

const BASE_URL = "http://localhost:9999";

export default function () {
  const correlationId = uuidv4();
  const payload = JSON.stringify({
    correlationId: correlationId,
    amount: Math.random() * 1000 + 1, // Random amount between 1 and 1001
  });

  const params = {
    headers: {
      "Content-Type": "application/json",
      Authorization: "Bearer 123",
    },
  };

  const response = http.post(`${BASE_URL}/payments`, payload, params);

  check(response, {
    "status is 200": (r) => r.status === 200,
    "response time < 2000ms": (r) => r.timings.duration < 2000,
    "has success message": (r) =>
      r.json().message === "payment processed successfully",
  });

  sleep(0.1); // Wait 100ms between requests
}

// Teste do endpoint de summary
export function teardown() {
  const response = http.get(`${BASE_URL}/payments-summary`);
  check(response, {
    "summary status is 200": (r) => r.status === 200,
    "has default and fallback stats": (r) => {
      const json = r.json();
      return (
        json.default &&
        json.fallback &&
        typeof json.default.total_requests === "number" &&
        typeof json.fallback.total_requests === "number"
      );
    },
  });
}
