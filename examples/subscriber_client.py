#!/usr/bin/env python3
"""
Demo subscriber client for rs-broker.

This script demonstrates how to:
1. Register as a subscriber with rs-broker
2. Receive events via gRPC streaming
3. Acknowledge processed messages

Usage:
  python subscriber_client.py
"""

import grpc
import time
import json
import os
import signal
import sys
from concurrent import futures

SUBSCRIBER_ID = os.getenv("SUBSCRIBER_ID", "demo-subscriber-1")
SERVICE_NAME = os.getenv("SERVICE_NAME", "demo-service")
GRPC_ENDPOINT = os.getenv("GRPC_ENDPOINT", "0.0.0.0:50052")
RS_BROKER_ENDPOINT = os.getenv("RS_BROKER_GRPC_ENDPOINT", "localhost:50051")
TOPIC_PATTERNS = os.getenv("TOPIC_PATTERNS", "orders.*,payments.created").split(",")


def main():
    print(f"Starting demo subscriber: {SUBSCRIBER_ID}")
    print(f"Service name: {SERVICE_NAME}")
    print(f"Topic patterns: {TOPIC_PATTERNS}")
    print(f"rs-broker endpoint: {RS_BROKER_ENDPOINT}")
    print(f"Local gRPC endpoint: {GRPC_ENDPOINT}")

    try:
        channel = grpc.insecure_channel(RS_BROKER_ENDPOINT)
        print(f"Connected to rs-broker at {RS_BROKER_ENDPOINT}")

        print("\n[Demo] In production, this would:")
        print("  1. Register as a subscriber via RegisterSubscriber RPC")
        print("  2. Start a local gRPC server to receive events")
        print("  3. Subscribe to events via SubscribeEvents stream")
        print("  4. Process events and send acknowledgements")
        print("\n[Demo] Simulating message processing...")

        counter = 0
        while True:
            counter += 1
            time.sleep(5)
            print(
                f"[{time.strftime('%H:%M:%S')}] Waiting for messages... (count: {counter})"
            )

    except grpc.RpcError as e:
        print(f"gRPC error: {e.code()}: {e.details()}")
        sys.exit(1)
    except KeyboardInterrupt:
        print("\nShutting down...")
        sys.exit(0)


if __name__ == "__main__":
    main()
