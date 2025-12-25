package main

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"

	// "github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

const (
	serverPort = ":3000"
)

// HelloWorldServiceServer implements the HelloWorldService
type HelloWorldServiceServer struct{}

// SayHelloStream implements server-side streaming
// This handler demonstrates CORRECT error handling for Connect streaming:
// - Errors returned before streaming starts are properly framed as EndStream
// - The Connect library handles this correctly, returning HTTP 200 with error in EndStream frame
func (s *HelloWorldServiceServer) SayHelloStream(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
	stream *connect.ServerStream[gen.HelloResponse],
) error {
	name := "there"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	log.Printf("📥 Received SayHelloStream request from: %s", name)

	// Simulate authorization check that fails BEFORE streaming
	if name == "unauthorized" {
		log.Printf("❌ Authorization failed for: %s", name)
		return connect.NewError(connect.CodePermissionDenied, fmt.Errorf("you are not authorized to access this stream"))
	}

	// Simulate input validation failure BEFORE streaming
	if name == "invalid" {
		log.Printf("❌ Validation failed for: %s", name)
		return connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("invalid name provided"))
	}

	// Simulate resource not found BEFORE streaming
	if name == "notfound" {
		log.Printf("❌ Resource not found for: %s", name)
		return connect.NewError(connect.CodeNotFound, fmt.Errorf("requested resource does not exist"))
	}

	// Normal case: stream messages
	messages := []string{
		fmt.Sprintf("Hello, %s! This is message 1.", name),
		fmt.Sprintf("Hello, %s! This is message 2.", name),
		fmt.Sprintf("Hello, %s! This is message 3.", name),
	}

	for i, msg := range messages {
		log.Printf("📤 Sending message %d: %s", i+1, msg)

		if err := stream.Send(&gen.HelloResponse{
			Message: msg,
		}); err != nil {
			log.Printf("❌ Error sending message %d: %v", i+1, err)
			return err
		}

		// Small delay between messages to simulate processing
		time.Sleep(100 * time.Millisecond)
	}

	log.Printf("✅ Stream completed successfully")
	return nil
}

// SayHello implements unary RPC (required by interface, but not used for comparison)
func (s *HelloWorldServiceServer) SayHello(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	name := "there"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	msg := fmt.Sprintf("Hello, %s!", name)
	return connect.NewResponse(&gen.HelloResponse{
		Message: msg,
	}), nil
}

func main() {
	server := &HelloWorldServiceServer{}

	// Create the Connect handler
	path, handler := genconnect.NewHelloWorldServiceHandler(server)

	mux := http.NewServeMux()
	mux.Handle(path, handler)

	// Add a health check endpoint
	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("OK"))
	})

	log.Printf("🚀 Go Connect server starting on %s", serverPort)
	log.Printf("📍 Endpoint: %s", path)
	log.Printf("🔧 Implementation: SayHelloStream with early error handling")
	log.Printf("")
	log.Printf("Error test cases (these return errors BEFORE streaming):")
	log.Printf("  - name='unauthorized' -> PermissionDenied")
	log.Printf("  - name='invalid'      -> InvalidArgument")
	log.Printf("  - name='notfound'     -> NotFound")
	log.Printf("")
	log.Printf("This server demonstrates CORRECT behavior:")
	log.Printf("  - HTTP 200 with application/connect+json")
	log.Printf("  - Error in EndStream frame")
	log.Printf("")
	log.Printf("Test with: go run ./cmd/client stream-error")

	// Use h2c for HTTP/2 without TLS
	err := http.ListenAndServe(
		serverPort,
		h2c.NewHandler(mux, &http2.Server{}),
	)

	if err != nil {
		log.Fatalf("Failed to start server: %v", err)
	}
}
