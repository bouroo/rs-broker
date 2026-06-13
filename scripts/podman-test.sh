#!/usr/bin/env bash
# =============================================================================
# podman-test.sh - Run rs-broker test suite inside a container
# =============================================================================
# Usage:
#   ./scripts/podman-test.sh              # run the full test suite
#   ./scripts/podman-test.sh -- logs      # follow test container logs
#
# Auto-detects podman or docker. Always tears down dependencies on exit.
# =============================================================================
set -euo pipefail

# ----------------------------------------------------------------------------
# Container runtime detection (prefer podman)
# ----------------------------------------------------------------------------
detect_compose_cmd() {
    if command -v podman >/dev/null 2>&1; then
        if podman compose version >/dev/null 2>&1; then
            echo "podman compose"
            return
        fi
    fi
    if command -v docker >/dev/null 2>&1; then
        if docker compose version >/dev/null 2>&1; then
            echo "docker compose"
            return
        fi
    fi
    echo ""
}

COMPOSE_CMD=$(detect_compose_cmd)
if [[ -z "${COMPOSE_CMD}" ]]; then
    echo "ERROR: neither 'podman compose' nor 'docker compose' is available." >&2
    exit 127
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="${SCRIPT_DIR}/../compose.test.yml"
COMPOSE_FILE="$(cd "$(dirname "${COMPOSE_FILE}")" && pwd)/$(basename "${COMPOSE_FILE}")"

# ----------------------------------------------------------------------------
# Logs subcommand: keep dependencies up and stream test container output
# ----------------------------------------------------------------------------
if [[ "${1:-}" == "logs" ]]; then
    "${COMPOSE_CMD}" -f "${COMPOSE_FILE}" up -d postgres kafka
    trap "${COMPOSE_CMD} -f '${COMPOSE_FILE}' down -v" EXIT INT TERM
    "${COMPOSE_CMD}" -f "${COMPOSE_FILE}" logs -f test
    exit 0
fi

# ----------------------------------------------------------------------------
# Health-check wait with countdown (max 120s)
# ----------------------------------------------------------------------------
wait_for_healthy() {
    local service="$1"
    local timeout=120
    local elapsed=0
    echo "==> Waiting for ${service} to become healthy..."
    while (( elapsed < timeout )); do
        local state
        state="$(${COMPOSE_CMD} -f "${COMPOSE_FILE}" ps --format '{{.Service}} {{.Health}}' "${service}" 2>/dev/null || true)"
        if [[ "${state}" == *"healthy"* ]]; then
            echo "==> ${service} is healthy"
            return 0
        fi
        sleep 2
        elapsed=$((elapsed + 2))
        printf "  ... %ds/%ds\n" "${elapsed}" "${timeout}"
    done
    echo "ERROR: ${service} did not become healthy within ${timeout}s" >&2
    return 1
}

# ----------------------------------------------------------------------------
# Main: up deps -> wait -> run tests -> down (always)
# ----------------------------------------------------------------------------
TEST_EXIT=0

cleanup() {
    echo "==> Tearing down test environment..."
    ${COMPOSE_CMD} -f "${COMPOSE_FILE}" down -v >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

echo "==> Using: ${COMPOSE_CMD}"
echo "==> Starting postgres + kafka..."
${COMPOSE_CMD} -f "${COMPOSE_FILE}" up -d postgres kafka

wait_for_healthy postgres
wait_for_healthy kafka

echo "==> Running test suite..."
set +e
${COMPOSE_CMD} -f "${COMPOSE_FILE}" run --rm test
TEST_EXIT=$?
set -e

echo "==> Tests exited with code ${TEST_EXIT}"
exit "${TEST_EXIT}"
