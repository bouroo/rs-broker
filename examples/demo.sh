#!/bin/bash
# =============================================================================
# rs-broker Demo Script
# =============================================================================
# Demonstrates essential features of rs-broker using grpcurl
#
# Prerequisites:
#   - grpcurl installed (https://github.com/fullstorydev/grpcurl)
#   - rs-broker running (docker-compose up -d)
#
# Usage:
#   ./demo.sh
# =============================================================================

set -e

GRPC_HOST="${GRPC_HOST:-localhost:50051}"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_grpcurl() {
    if ! command -v grpcurl &> /dev/null; then
        log_error "grpcurl is not installed"
        log_info "Install with: go install github.com/fullstorydev/grpcurl/cmd/grpcurl@latest"
        exit 1
    fi
    log_info "grpcurl found: $(grpcurl --version)"
}

check_connection() {
    log_info "Checking connection to rs-broker at ${GRPC_HOST}..."
    if grpcurl -plaintext "${GRPC_HOST}" list &> /dev/null; then
        log_info "Connected successfully"
    else
        log_error "Cannot connect to rs-broker at ${GRPC_HOST}"
        log_info "Make sure rs-broker is running: docker-compose up -d"
        exit 1
    fi
}

list_services() {
    log_info "Available gRPC services:"
    grpcurl -plaintext "${GRPC_HOST}" list
}

publish_message() {
    local aggregate_type="${1:-Order}"
    local aggregate_id="${2:-order-$(date +%s)}"
    local event_type="${3:-OrderCreated}"
    local topic="${4:-orders}"

    log_info "Publishing ${event_type} event for ${aggregate_type}:${aggregate_id} to topic '${topic}'"

    grpcurl -plaintext -d "{
        \"aggregate_type\": \"${aggregate_type}\",
        \"aggregate_id\": \"${aggregate_id}\",
        \"event_type\": \"${event_type}\",
        \"payload\": \"{\\\"orderId\\\": \\\"${aggregate_id}\\\", \\\"amount\\\": 99.99, \\\"currency\\\": \\\"USD\\\", \\\"timestamp\\\": \\\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\\\"}\",
        \"topic\": \"${topic}\",
        \"headers\": {
            \"source\": \"demo-script\",
            \"version\": \"1.0\"
        }
    }" "${GRPC_HOST}" rsbroker.RsBroker/Publish

    log_info "Message published successfully"
}

register_subscriber() {
    local subscriber_id="${1:-demo-subscriber-1}"
    local service_name="${2:-demo-service}"
    local grpc_endpoint="${3:-localhost:50052}"

    log_info "Registering subscriber: ${subscriber_id}"

    grpcurl -plaintext -d "{
        \"subscriber_id\": \"${subscriber_id}\",
        \"service_name\": \"${service_name}\",
        \"grpc_endpoint\": \"${grpc_endpoint}\",
        \"topic_patterns\": [\"orders.*\", \"payments.created\"]
    }" "${GRPC_HOST}" rsbroker.RsBroker/RegisterSubscriber

    log_info "Subscriber registered"
}

get_subscriber() {
    local subscriber_id="${1:-demo-subscriber-1}"

    log_info "Getting subscriber: ${subscriber_id}"

    grpcurl -plaintext -d "{
        \"subscriber_id\": \"${subscriber_id}\"
    }" "${GRPC_HOST}" rsbroker.RsBroker/GetSubscriber
}

list_subscribers() {
    log_info "Listing all subscribers"
    grpcurl -plaintext "${GRPC_HOST}" rsbroker.RsBroker/ListSubscribers
}

subscribe_events() {
    local subscriber_id="${1:-demo-subscriber-1}"

    log_info "Subscribing to events (press Ctrl+C to stop)..."
    log_warn "Note: This is a streaming RPC that will block"

    grpcurl -plaintext -d "{
        \"subscriber_id\": \"${subscriber_id}\",
        \"topic_patterns\": [\"orders.*\"],
        \"position\": \"LATEST\"
    }" "${GRPC_HOST}" rsbroker.RsBroker/SubscribeEvents
}

demo_publish_batch() {
    log_info "Publishing batch of messages..."

    for i in {1..5}; do
        publish_message "Order" "order-batch-${i}-$(date +%s)" "OrderCreated" "orders"
        sleep 0.5
    done

    for i in {1..3}; do
        publish_message "Payment" "payment-batch-${i}-$(date +%s)" "PaymentCreated" "payments"
        sleep 0.5
    done

    log_info "Batch publishing complete"
}

demo_dlq() {
    log_info "Demonstrating Dead Letter Queue..."
    log_warn "Messages that fail after max retries will be routed to DLQ topic"

    publish_message "Order" "order-dlq-test-$(date +%s)" "OrderFailed" "orders"
}

print_usage() {
    echo "rs-broker Demo Script"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  check         Check connection to rs-broker"
    echo "  services      List available gRPC services"
    echo "  publish       Publish a single test message"
    echo "  batch         Publish a batch of test messages"
    echo "  register      Register a demo subscriber"
    echo "  subscribers   List all registered subscribers"
    echo "  get           Get subscriber details"
    echo "  subscribe     Subscribe to events (streaming)"
    echo "  dlq           Demo Dead Letter Queue"
    echo "  full          Run full demo (publish + register)"
    echo "  help          Show this help message"
}

run_full_demo() {
    log_info "Running full demo..."
    check_connection
    list_services

    log_info "Step 1: Publishing test messages"
    publish_message

    log_info "Step 2: Registering subscriber"
    register_subscriber

    log_info "Step 3: Listing subscribers"
    list_subscribers

    log_info "Demo complete!"
}

main() {
    local command="${1:-help}"

    case "${command}" in
        check)
            check_grpcurl
            check_connection
            ;;
        services)
            check_grpcurl
            check_connection
            list_services
            ;;
        publish)
            check_grpcurl
            check_connection
            publish_message "${2}" "${3}" "${4}" "${5}"
            ;;
        batch)
            check_grpcurl
            check_connection
            demo_publish_batch
            ;;
        register)
            check_grpcurl
            check_connection
            register_subscriber "${2}" "${3}" "${4}"
            ;;
        subscribers|list)
            check_grpcurl
            check_connection
            list_subscribers
            ;;
        get)
            check_grpcurl
            check_connection
            get_subscriber "${2}"
            ;;
        subscribe)
            check_grpcurl
            check_connection
            subscribe_events "${2}"
            ;;
        dlq)
            check_grpcurl
            check_connection
            demo_dlq
            ;;
        full)
            check_grpcurl
            run_full_demo
            ;;
        help|--help|-h)
            print_usage
            ;;
        *)
            log_error "Unknown command: ${command}"
            print_usage
            exit 1
            ;;
    esac
}

main "$@"
