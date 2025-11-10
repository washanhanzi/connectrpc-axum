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

	// Send 3 messages with a small delay between them
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
	log.Printf("🔧 Implementation: SayHelloStream only (for comparison)")
	log.Printf("")
	log.Printf("Test with: go run main.go (update serverURL to http://localhost:3000)")

	// Use h2c for HTTP/2 without TLS
	err := http.ListenAndServe(
		serverPort,
		h2c.NewHandler(mux, &http2.Server{}),
	)

	if err != nil {
		log.Fatalf("Failed to start server: %v", err)
	}
}
