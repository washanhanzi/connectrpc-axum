// Package main provides integration tests for connectrpc-axum.
//
// These tests start Rust servers and validate they work correctly with Go clients
// using Connect, gRPC, and gRPC-Web protocols.
//
// Run: go test -v
// Run single: go test -v -run TestConnectUnary
package main

import (
	"bytes"
	"context"
	"fmt"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

const (
	serverPort     = "3000"
	serverAddr     = "localhost:" + serverPort
	serverURL      = "http://" + serverAddr
	maxWaitTime    = 30 * time.Second
	pollInterval   = 200 * time.Millisecond
)

var (
	examplesDir string
	rootDir     string
	buildOnce   sync.Once
	buildErr    error
)

func TestMain(m *testing.M) {
	// Find project root
	wd, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Failed to get working directory: %v\n", err)
		os.Exit(1)
	}

	// go-client is inside connectrpc-axum-examples
	examplesDir = filepath.Dir(wd)
	rootDir = filepath.Dir(examplesDir)

	os.Exit(m.Run())
}

// buildServers builds all example servers once before running tests
func buildServers(t *testing.T) {
	buildOnce.Do(func() {
		t.Log("Building all example servers...")
		cmd := exec.Command("cargo", "build", "-p", "connectrpc-axum-examples", "--features", "tonic")
		cmd.Dir = rootDir

		var stderr bytes.Buffer
		cmd.Stderr = &stderr

		if err := cmd.Run(); err != nil {
			buildErr = fmt.Errorf("cargo build failed: %v\nstderr: %s", err, stderr.String())
		}
	})

	if buildErr != nil {
		t.Fatal(buildErr)
	}
}

// server manages a Rust server process
type server struct {
	name     string
	features string
	cmd      *exec.Cmd
	cancel   context.CancelFunc
}

// startServer starts a Rust server and waits for it to be ready
func startServer(t *testing.T, name, features string) *server {
	buildServers(t)

	ctx, cancel := context.WithCancel(context.Background())

	args := []string{"run", "-p", "connectrpc-axum-examples", "--bin", name}
	if features != "" {
		args = append(args, "--features", features)
	}

	cmd := exec.CommandContext(ctx, "cargo", args...)
	cmd.Dir = rootDir
	cmd.Stdout = nil // Discard output
	cmd.Stderr = nil

	if err := cmd.Start(); err != nil {
		cancel()
		t.Fatalf("Failed to start server %s: %v", name, err)
	}

	s := &server{
		name:     name,
		features: features,
		cmd:      cmd,
		cancel:   cancel,
	}

	// Wait for server to be ready
	if !waitForServer(t, maxWaitTime) {
		s.stop()
		t.Fatalf("Server %s failed to start within %v", name, maxWaitTime)
	}

	t.Logf("Server %s ready (PID: %d)", name, cmd.Process.Pid)
	return s
}

// stop gracefully stops the server
func (s *server) stop() {
	s.cancel()
	// Wait briefly for clean shutdown
	done := make(chan struct{})
	go func() {
		s.cmd.Wait()
		close(done)
	}()

	select {
	case <-done:
	case <-time.After(2 * time.Second):
		s.cmd.Process.Kill()
	}
}

// waitForServer polls until the server is accepting connections
func waitForServer(t *testing.T, timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)

	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", serverAddr, pollInterval)
		if err == nil {
			conn.Close()
			return true
		}
		time.Sleep(pollInterval)
	}
	return false
}

// runGoClient runs the Go client with given arguments and returns output
func runGoClient(t *testing.T, protocol, command string) (string, error) {
	args := []string{"run", "./cmd/client", "-server", serverURL, "-protocol", protocol, command}
	cmd := exec.Command("go", args...)
	cmd.Dir = filepath.Join(examplesDir, "go-client")

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	output := stdout.String() + stderr.String()

	if err != nil {
		return output, fmt.Errorf("client failed: %v\noutput: %s", err, output)
	}
	return output, nil
}

// runGoClientCommand runs the Go client with raw arguments
func runGoClientCommand(t *testing.T, args ...string) (string, error) {
	fullArgs := append([]string{"run", "./cmd/client", "-server", serverURL}, args...)
	cmd := exec.Command("go", fullArgs...)
	cmd.Dir = filepath.Join(examplesDir, "go-client")

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()
	output := stdout.String() + stderr.String()

	if err != nil {
		return output, fmt.Errorf("client failed: %v\noutput: %s", err, output)
	}
	return output, nil
}

// ============================================================================
// Test 1: connect-unary (Connect protocol only)
// ============================================================================

func TestConnectUnary(t *testing.T) {
	s := startServer(t, "connect-unary", "")
	defer s.stop()

	output, err := runGoClient(t, "connect", "unary")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Response:") {
		t.Errorf("Expected response output, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 2: connect-server-stream (Connect protocol only)
// ============================================================================

func TestConnectServerStream(t *testing.T) {
	s := startServer(t, "connect-server-stream", "")
	defer s.stop()

	output, err := runGoClient(t, "connect", "server-stream")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Received") || !strings.Contains(output, "messages") {
		t.Errorf("Expected streaming output, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 3: tonic-unary (Connect + gRPC)
// ============================================================================

func TestTonicUnaryConnect(t *testing.T) {
	s := startServer(t, "tonic-unary", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "connect", "unary")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Response:") {
		t.Errorf("Expected response output, got: %s", output)
	}
	t.Log(output)
}

func TestTonicUnaryGRPC(t *testing.T) {
	s := startServer(t, "tonic-unary", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "grpc", "unary")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Response:") {
		t.Errorf("Expected response output, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 4: tonic-server-stream (Connect + gRPC)
// ============================================================================

func TestTonicServerStreamConnect(t *testing.T) {
	s := startServer(t, "tonic-server-stream", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "connect", "server-stream")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Received") || !strings.Contains(output, "messages") {
		t.Errorf("Expected streaming output, got: %s", output)
	}
	t.Log(output)
}

func TestTonicServerStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-server-stream", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "grpc", "server-stream")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Received") || !strings.Contains(output, "messages") {
		t.Errorf("Expected streaming output, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 5: tonic-bidi-stream (gRPC for bidi, Connect for unary)
// ============================================================================

func TestTonicBidiStreamConnectUnary(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "connect", "unary")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Response:") {
		t.Errorf("Expected response output, got: %s", output)
	}
	t.Log(output)
}

func TestTonicBidiStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	output, err := runGoClient(t, "grpc", "bidi-stream")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Bidi stream completed") {
		t.Errorf("Expected bidi completion output, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 6: grpc-web
// ============================================================================

func TestGRPCWeb(t *testing.T) {
	s := startServer(t, "grpc-web", "tonic-web")
	defer s.stop()

	output, err := runGoClientCommand(t, "grpc-web")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Response message:") {
		t.Errorf("Expected gRPC-Web response, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 7: streaming-error-repro (Streaming error handling)
// ============================================================================

func TestStreamingErrorHandling(t *testing.T) {
	s := startServer(t, "streaming-error-repro", "")
	defer s.stop()

	output, err := runGoClient(t, "connect", "stream-error")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "stream error tests passed") {
		t.Errorf("Expected error handling test pass, got: %s", output)
	}
	t.Log(output)
}

// ============================================================================
// Test 8: protocol-version (Connect-Protocol-Version header validation)
// ============================================================================

func TestProtocolVersion(t *testing.T) {
	s := startServer(t, "protocol-version", "")
	defer s.stop()

	output, err := runGoClientCommand(t, "protocol-version")
	if err != nil {
		t.Fatal(err)
	}

	if !strings.Contains(output, "Protocol version test passed") {
		t.Errorf("Expected protocol version test pass, got: %s", output)
	}
	t.Log(output)
}
