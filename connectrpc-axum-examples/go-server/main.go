// Go Reference Server for connectrpc-axum cross-implementation testing.
//
// This server implements all proto services using connect-go, supporting both
// Connect and gRPC protocols. It serves as the reference implementation for
// testing the Rust client.
//
// Usage:
//
//	go run .                    # Start on default port 3000
//	PORT=4000 go run .          # Start on custom port
//	SERVER_URL=... go run .     # Alternative env var (parsed for port)
package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"os"
	"strings"
	"sync/atomic"
	"time"

	"connectrpc.com/connect"
	"connectrpc.com/grpcreflect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

// Global request counter for testing stateful behavior
var requestCounter atomic.Int64

// =============================================================================
// HelloWorldService Implementation
// =============================================================================

type helloWorldServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

// SayHello implements unary RPC
func (s *helloWorldServer) SayHello(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	msg := fmt.Sprintf("Hello, %s!", name)
	return connect.NewResponse(&gen.HelloResponse{
		Message: msg,
	}), nil
}

// SayHelloStream implements server-side streaming RPC
func (s *helloWorldServer) SayHelloStream(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
	stream *connect.ServerStream[gen.HelloResponse],
) error {
	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	// Test error cases (return error before streaming)
	switch name {
	case "unauthorized":
		return connect.NewError(connect.CodePermissionDenied, errors.New("you are not authorized"))
	case "invalid":
		return connect.NewError(connect.CodeInvalidArgument, errors.New("invalid name provided"))
	case "notfound":
		return connect.NewError(connect.CodeNotFound, errors.New("resource not found"))
	}

	// First greeting (matches Rust server format)
	if err := stream.Send(&gen.HelloResponse{
		Message: fmt.Sprintf("Hello, %s! Starting stream...", name),
	}); err != nil {
		return err
	}

	// Stream hobbies if provided (matches Rust server format)
	if len(req.Msg.Hobbies) > 0 {
		for idx, hobby := range req.Msg.Hobbies {
			if err := stream.Send(&gen.HelloResponse{
				Message: fmt.Sprintf("Hobby #%d: %s - nice!", idx+1, hobby),
			}); err != nil {
				return err
			}
			time.Sleep(10 * time.Millisecond)
		}
	} else {
		// Send sample messages (matches Rust server format)
		for i := 1; i <= 3; i++ {
			if err := stream.Send(&gen.HelloResponse{
				Message: fmt.Sprintf("Stream message #%d for %s", i, name),
			}); err != nil {
				return err
			}
			time.Sleep(10 * time.Millisecond)
		}
	}

	// Final message (matches Rust server format)
	if err := stream.Send(&gen.HelloResponse{
		Message: fmt.Sprintf("Stream complete. Goodbye, %s!", name),
	}); err != nil {
		return err
	}

	return nil
}

// GetGreeting implements idempotent unary RPC (supports HTTP GET)
func (s *helloWorldServer) GetGreeting(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	count := requestCounter.Add(1)
	msg := fmt.Sprintf("Greetings #%d, %s!", count, name)

	return connect.NewResponse(&gen.HelloResponse{
		Message: msg,
	}), nil
}

// =============================================================================
// EchoService Implementation
// =============================================================================

type echoServer struct {
	genconnect.UnimplementedEchoServiceHandler
}

// Echo implements unary RPC
func (s *echoServer) Echo(
	ctx context.Context,
	req *connect.Request[gen.EchoRequest],
) (*connect.Response[gen.EchoResponse], error) {
	count := requestCounter.Add(1)
	msg := fmt.Sprintf("Echo #%d: %s", count, req.Msg.Message)

	return connect.NewResponse(&gen.EchoResponse{
		Message: msg,
	}), nil
}

// EchoClientStream implements client-side streaming RPC
func (s *echoServer) EchoClientStream(
	ctx context.Context,
	stream *connect.ClientStream[gen.EchoRequest],
) (*connect.Response[gen.EchoResponse], error) {
	var messages []string

	for stream.Receive() {
		messages = append(messages, stream.Msg().Message)
	}

	if err := stream.Err(); err != nil {
		return nil, err
	}

	// Format matches Rust server
	msg := fmt.Sprintf("Client Stream Complete: Received %d messages: [%s]",
		len(messages), strings.Join(messages, ", "))

	return connect.NewResponse(&gen.EchoResponse{
		Message: msg,
	}), nil
}

// EchoBidiStream implements bidirectional streaming RPC
func (s *echoServer) EchoBidiStream(
	ctx context.Context,
	stream *connect.BidiStream[gen.EchoRequest, gen.EchoResponse],
) error {
	messageCount := 0

	for {
		req, err := stream.Receive()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return err
		}

		messageCount++
		count := requestCounter.Add(1)

		// Echo back each message immediately
		if err := stream.Send(&gen.EchoResponse{
			Message: fmt.Sprintf("Bidi Echo #%d (msg #%d): %s", count, messageCount, req.Message),
		}); err != nil {
			return err
		}
	}

	// Send final summary message (format matches Rust server)
	count := requestCounter.Add(1)
	return stream.Send(&gen.EchoResponse{
		Message: fmt.Sprintf("Bidi stream #%d completed. Echoed %d messages.", count, messageCount),
	})
}

// =============================================================================
// Server Setup
// =============================================================================

func getPort() string {
	// Check PORT env var first
	if port := os.Getenv("PORT"); port != "" {
		return port
	}

	// Check SERVER_URL and extract port
	if serverURL := os.Getenv("SERVER_URL"); serverURL != "" {
		if u, err := url.Parse(serverURL); err == nil {
			if port := u.Port(); port != "" {
				return port
			}
		}
	}

	// Default port
	return "3000"
}

func main() {
	port := getPort()
	addr := ":" + port

	mux := http.NewServeMux()

	// Register HelloWorldService
	helloPath, helloHandler := genconnect.NewHelloWorldServiceHandler(&helloWorldServer{})
	mux.Handle(helloPath, helloHandler)

	// Register EchoService
	echoPath, echoHandler := genconnect.NewEchoServiceHandler(&echoServer{})
	mux.Handle(echoPath, echoHandler)

	// Register gRPC reflection for debugging with grpcurl
	reflector := grpcreflect.NewStaticReflector(
		genconnect.HelloWorldServiceName,
		genconnect.EchoServiceName,
	)
	reflectPath1, reflectHandler1 := grpcreflect.NewHandlerV1(reflector)
	mux.Handle(reflectPath1, reflectHandler1)
	reflectPath2, reflectHandler2 := grpcreflect.NewHandlerV1Alpha(reflector)
	mux.Handle(reflectPath2, reflectHandler2)

	// Health check endpoint
	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("OK"))
	})

	// Server ID endpoint for test verification
	mux.HandleFunc("/__server_id", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("go-server"))
	})

	log.Printf("=== Go Reference Server ===")
	log.Printf("Listening on %s", addr)
	log.Printf("")
	log.Printf("Services:")
	log.Printf("  HelloWorldService: %s", helloPath)
	log.Printf("    - SayHello (unary)")
	log.Printf("    - SayHelloStream (server streaming)")
	log.Printf("    - GetGreeting (unary, idempotent)")
	log.Printf("  EchoService: %s", echoPath)
	log.Printf("    - Echo (unary)")
	log.Printf("    - EchoClientStream (client streaming)")
	log.Printf("    - EchoBidiStream (bidi streaming)")
	log.Printf("")
	log.Printf("Protocols: Connect, gRPC, gRPC-Web")
	log.Printf("")
	log.Printf("Test with:")
	log.Printf("  curl -X POST http://localhost%s/hello.HelloWorldService/SayHello \\", addr)
	log.Printf("    -H 'Content-Type: application/json' -d '{\"name\":\"World\"}'")

	// Use h2c for HTTP/2 without TLS (required for gRPC)
	server := &http.Server{
		Addr:    addr,
		Handler: h2c.NewHandler(mux, &http2.Server{}),
	}

	if err := server.ListenAndServe(); err != nil {
		log.Fatalf("Failed to start server: %v", err)
	}
}
