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
	"sync"
	"testing"
	"time"
)

const (
	serverPort   = "3000"
	serverAddr   = "localhost:" + serverPort
	serverURL    = "http://" + serverAddr
	maxWaitTime  = 30 * time.Second
	pollInterval = 200 * time.Millisecond
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

