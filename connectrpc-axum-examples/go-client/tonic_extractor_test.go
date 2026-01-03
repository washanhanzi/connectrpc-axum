package main

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

// TestTonicExtractorConnect tests tonic server with multiple extractors via Connect protocol.
func TestTonicExtractorConnect(t *testing.T) {
	s := startServer(t, "tonic-extractor", "tonic")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	t.Run("without x-api-key should fail", func(t *testing.T) {
		name := "Alice"
		_, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		}))

		if err == nil {
			t.Fatal("Expected error when x-api-key header is missing")
		}

		connectErr := new(connect.Error)
		if !errors.As(err, &connectErr) {
			t.Fatalf("Expected connect.Error, got: %T - %v", err, err)
		}

		if connectErr.Code() != connect.CodeUnauthenticated {
			t.Fatalf("Expected CodeUnauthenticated, got: %v", connectErr.Code())
		}

		t.Logf("Got expected Connect error: code=%v message=%q", connectErr.Code(), connectErr.Message())
	})

	t.Run("with x-api-key should succeed", func(t *testing.T) {
		name := "Alice"
		req := connect.NewRequest(&gen.HelloRequest{
			Name: &name,
		})
		req.Header().Set("x-api-key", "secret123")

		resp, err := client.SayHello(context.Background(), req)
		if err != nil {
			t.Fatalf("Request with header failed: %v", err)
		}

		// Should contain name, counter, and api key
		if !strings.Contains(resp.Msg.Message, "Alice") {
			t.Fatalf("Response should contain name: %q", resp.Msg.Message)
		}
		if !strings.Contains(resp.Msg.Message, "secret123") {
			t.Fatalf("Response should contain api_key: %q", resp.Msg.Message)
		}
		if !strings.Contains(resp.Msg.Message, "#") {
			t.Fatalf("Response should contain counter: %q", resp.Msg.Message)
		}

		t.Logf("Response: %s", resp.Msg.Message)
	})
}

// TestTonicExtractorGRPC tests tonic server with multiple extractors via gRPC protocol.
func TestTonicExtractorGRPC(t *testing.T) {
	s := startServer(t, "tonic-extractor", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)

	t.Run("without x-api-key should fail", func(t *testing.T) {
		name := "Bob"
		_, err := client.SayHello(context.Background(), &gen.HelloRequest{
			Name: &name,
		})

		if err == nil {
			t.Fatal("Expected error when x-api-key metadata is missing")
		}

		// Should be gRPC UNAUTHENTICATED error
		if !strings.Contains(err.Error(), "Unauthenticated") {
			t.Fatalf("Expected Unauthenticated error, got: %v", err)
		}

		t.Logf("Got expected gRPC error: %v", err)
	})

	t.Run("with x-api-key should succeed", func(t *testing.T) {
		name := "Bob"
		// Add x-api-key as gRPC metadata
		ctx := metadata.AppendToOutgoingContext(context.Background(), "x-api-key", "grpc-secret")

		resp, err := client.SayHello(ctx, &gen.HelloRequest{
			Name: &name,
		})
		if err != nil {
			t.Fatalf("Request with metadata failed: %v", err)
		}

		// Should contain name, counter, and api key
		if !strings.Contains(resp.Message, "Bob") {
			t.Fatalf("Response should contain name: %q", resp.Message)
		}
		if !strings.Contains(resp.Message, "grpc-secret") {
			t.Fatalf("Response should contain api_key: %q", resp.Message)
		}
		if !strings.Contains(resp.Message, "#") {
			t.Fatalf("Response should contain counter: %q", resp.Message)
		}

		t.Logf("Response: %s", resp.Message)
	})
}
