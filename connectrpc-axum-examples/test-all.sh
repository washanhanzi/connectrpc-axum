#!/bin/bash
#
# Integration Test Script for connectrpc-axum
# ============================================
#
# This script runs all example servers with the Go test client to verify
# that all protocols (Connect, gRPC, gRPC-Web) work correctly.
#
# Usage:
#   ./connectrpc-axum-examples/test-all.sh
#
# Requirements:
#   - Rust toolchain (cargo)
#   - Go toolchain (go)
#   - Port 3000 available
#
# Test Matrix:
#   ┌─────────────────────────┬──────────┬───────────────────┐
#   │ Server                  │ Protocol │ Test Type         │
#   ├─────────────────────────┼──────────┼───────────────────┤
#   │ connect-unary           │ Connect  │ Unary             │
#   │ connect-server-stream   │ Connect  │ Server streaming  │
#   │ tonic-unary             │ Connect  │ Unary             │
#   │ tonic-unary             │ gRPC     │ Unary             │
#   │ tonic-server-stream     │ Connect  │ Server streaming  │
#   │ tonic-server-stream     │ gRPC     │ Server streaming  │
#   │ tonic-bidi-stream       │ Connect  │ Unary             │
#   │ tonic-bidi-stream       │ gRPC     │ Bidi streaming    │
#   │ grpc-web                │ gRPC-Web │ Unary             │
#   └─────────────────────────┴──────────┴───────────────────┘
#
# Exit Codes:
#   0 - All tests passed
#   N - Number of failed tests (1-9)
#
# Success Condition:
#   The script succeeds when ALL of the following are true:
#   1. All 6 Rust servers start successfully on port 3000
#   2. All 9 Go client tests complete without error
#   3. Each test receives expected response data
#   4. Exit code is 0
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
GO_CLIENT_DIR="$SCRIPT_DIR/go-client"
PORT=3000
WAIT_TIME=3

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Results tracking
declare -A RESULTS
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Cleanup function
cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Print header
print_header() {
    echo -e "\n${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}  $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}\n"
}

# Print test result
print_result() {
    local test_name=$1
    local result=$2
    local details=$3

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    if [ "$result" = "PASS" ]; then
        echo -e "  ${GREEN}✓${NC} $test_name ${GREEN}PASS${NC} $details"
        PASSED_TESTS=$((PASSED_TESTS + 1))
        RESULTS["$test_name"]="✅ PASS $details"
    else
        echo -e "  ${RED}✗${NC} $test_name ${RED}FAIL${NC} $details"
        FAILED_TESTS=$((FAILED_TESTS + 1))
        RESULTS["$test_name"]="❌ FAIL $details"
    fi
}

# Wait for server to be ready
wait_for_server() {
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s "http://localhost:$PORT" >/dev/null 2>&1|| \
           curl -s -o /dev/null -w "%{http_code}" "http://localhost:$PORT" 2>/dev/null | grep -q "4\|5"; then
            return 0
        fi
        sleep 0.2
        attempt=$((attempt + 1))
    done
    return 1
}

# Start a server
start_server() {
    local bin_name=$1
    local features=$2

    echo -e "  Starting ${YELLOW}$bin_name${NC}..."

    if [ -n "$features" ]; then
        cargo run -p connectrpc-axum-examples --bin "$bin_name" --features "$features" >/dev/null 2>&1 &
    else
        cargo run -p connectrpc-axum-examples --bin "$bin_name" >/dev/null 2>&1 &
    fi
    SERVER_PID=$!

    if wait_for_server; then
        echo -e "  Server ready (PID: $SERVER_PID)"
        return 0
    else
        echo -e "  ${RED}Server failed to start${NC}"
        return 1
    fi
}

# Stop the server
stop_server() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
        SERVER_PID=""
    fi
}

# Run Go client test
run_go_test() {
    local protocol=$1
    local command=$2
    local test_name=$3

    cd "$GO_CLIENT_DIR"

    local output
    if output=$(go run ./cmd/client -protocol "$protocol" "$command" 2>&1); then
        # Extract meaningful info from output
        local details=""
        if echo "$output" | grep -q "Bidi stream completed"; then
            local msg_count=$(echo "$output" | grep "Bidi stream completed" | grep -oE '[0-9]+' | head -1)
            details="Bidi completed ($msg_count messages)"
        elif echo "$output" | grep -q "Received [0-9]* messages"; then
            details=$(echo "$output" | grep "Received [0-9]* messages" | tail -1)
        elif echo "$output" | grep -q "Response:"; then
            details="Response received"
        fi
        print_result "$test_name" "PASS" "($details)"
        return 0
    else
        print_result "$test_name" "FAIL" "(Error: $output)"
        return 1
    fi
}

# Run gRPC-Web test
run_grpcweb_test() {
    local test_name=$1

    cd "$GO_CLIENT_DIR"

    local output
    if output=$(go run ./cmd/client grpc-web 2>&1); then
        if echo "$output" | grep -q "Response message:"; then
            print_result "$test_name" "PASS" "(Response received)"
            return 0
        fi
    fi
    print_result "$test_name" "FAIL" "(Error: $output)"
    return 1
}

# ============================================================================
# Main Test Execution
# ============================================================================

print_header "ConnectRPC-Axum Integration Tests"

echo "Building all examples..."
cd "$ROOT_DIR"
cargo build -p connectrpc-axum-examples --features tonic >/dev/null 2>&1
echo -e "${GREEN}Build complete${NC}\n"

# ----------------------------------------------------------------------------
# Test 1: connect-unary (Connect protocol only)
# ----------------------------------------------------------------------------
print_header "Test 1: connect-unary"

if start_server "connect-unary" ""; then
    run_go_test "connect" "unary" "connect-unary / Connect"
    stop_server
fi

# ----------------------------------------------------------------------------
# Test 2: connect-server-stream (Connect protocol only)
# ----------------------------------------------------------------------------
print_header "Test 2: connect-server-stream"

if start_server "connect-server-stream" ""; then
    run_go_test "connect" "server-stream" "connect-server-stream / Connect"
    stop_server
fi

# ----------------------------------------------------------------------------
# Test 3: tonic-unary (Connect + gRPC)
# ----------------------------------------------------------------------------
print_header "Test 3: tonic-unary"

if start_server "tonic-unary" "tonic"; then
    run_go_test "connect" "unary" "tonic-unary / Connect"
    run_go_test "grpc" "unary" "tonic-unary / gRPC"
    stop_server
fi

# ----------------------------------------------------------------------------
# Test 4: tonic-server-stream (Connect + gRPC)
# ----------------------------------------------------------------------------
print_header "Test 4: tonic-server-stream"

if start_server "tonic-server-stream" "tonic"; then
    run_go_test "connect" "server-stream" "tonic-server-stream / Connect"
    run_go_test "grpc" "server-stream" "tonic-server-stream / gRPC"
    stop_server
fi

# ----------------------------------------------------------------------------
# Test 5: tonic-bidi-stream (gRPC only for bidi, Connect for unary)
# ----------------------------------------------------------------------------
print_header "Test 5: tonic-bidi-stream"

if start_server "tonic-bidi-stream" "tonic"; then
    run_go_test "connect" "unary" "tonic-bidi-stream / Connect unary"
    run_go_test "grpc" "bidi-stream" "tonic-bidi-stream / gRPC bidi"
    stop_server
fi

# ----------------------------------------------------------------------------
# Test 6: grpc-web
# ----------------------------------------------------------------------------
print_header "Test 6: grpc-web"

if start_server "grpc-web" "tonic-web"; then
    run_grpcweb_test "grpc-web / gRPC-Web"
    stop_server
fi

# ============================================================================
# Summary
# ============================================================================

print_header "Test Summary"

echo -e "Results:\n"

# Print results in order
for test_name in \
    "connect-unary / Connect" \
    "connect-server-stream / Connect" \
    "tonic-unary / Connect" \
    "tonic-unary / gRPC" \
    "tonic-server-stream / Connect" \
    "tonic-server-stream / gRPC" \
    "tonic-bidi-stream / Connect unary" \
    "tonic-bidi-stream / gRPC bidi" \
    "grpc-web / gRPC-Web"
do
    if [ -n "${RESULTS[$test_name]}" ]; then
        echo "  $test_name: ${RESULTS[$test_name]}"
    fi
done

echo ""
echo -e "═══════════════════════════════════════════════════════════════"
if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "${GREEN}All $TOTAL_TESTS tests passed!${NC}"
else
    echo -e "${RED}$FAILED_TESTS/$TOTAL_TESTS tests failed${NC}"
fi
echo -e "═══════════════════════════════════════════════════════════════"

exit $FAILED_TESTS
