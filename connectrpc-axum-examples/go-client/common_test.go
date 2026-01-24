// Package main provides integration tests for connectrpc-axum.
//
// These tests start Rust servers and validate they work correctly with Go clients
// using Connect, gRPC, and gRPC-Web protocols.
//
// Run: go test -v
// Run single: go test -v -run TestConnectUnary
//
// With custom server URL (for use with Rust test runner):
//
//	SERVER_URL=http://localhost:4567 go test -v -run TestConnectUnary
package main

import (
	"bytes"
	"context"
	"fmt"
	"net"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

const (
	defaultServerPort = "3000"
	maxWaitTime       = 30 * time.Second
	pollInterval      = 200 * time.Millisecond
)

var (
	// serverURL is the base URL for the test server.
	// Set via SERVER_URL env var, defaults to http://localhost:3000
	serverURL string

	// serverAddr is the host:port for TCP connections (derived from serverURL)
	serverAddr string

	examplesDir string
	rootDir     string
	buildOnce   sync.Once
	buildErr    error

	// useExternalServer indicates whether SERVER_URL was set externally.
	// When true, tests skip starting their own server.
	useExternalServer bool
)

func init() {
	// Initialize server URL from environment or use default
	serverURL = os.Getenv("SERVER_URL")
	if serverURL == "" {
		serverURL = "http://localhost:" + defaultServerPort
	}

	// Parse server address from URL
	parsed, err := url.Parse(serverURL)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Invalid SERVER_URL: %v\n", err)
		os.Exit(1)
	}
	serverAddr = parsed.Host

	// If SERVER_URL was explicitly set, use external server mode
	useExternalServer = os.Getenv("SERVER_URL") != ""
}

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
	if useExternalServer {
		return // Skip building when using external server
	}

	buildOnce.Do(func() {
		t.Log("Building all example servers...")
		// Build with tonic feature to cover all examples
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
	external bool // true if using external server (no process to manage)
}

// startServer starts a Rust server and waits for it to be ready.
// If SERVER_URL is set externally, it skips starting a server and just verifies connectivity.
func startServer(t *testing.T, name, features string) *server {
	if useExternalServer {
		// External server mode: just verify the server is reachable
		if !waitForServer(t, 5*time.Second) {
			t.Fatalf("External server at %s is not reachable", serverURL)
		}
		t.Logf("Using external server at %s", serverURL)
		return &server{name: name, external: true}
	}

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

	// Set PORT environment variable to match serverAddr
	port := strings.Split(serverAddr, ":")[1]
	cmd.Env = append(os.Environ(), "PORT="+port)

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
	if s.external {
		return // Nothing to stop for external servers
	}

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
