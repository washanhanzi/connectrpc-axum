package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/test/go/gen"
	"github.com/connectrpc-axum/test/go/gen/genconnect"
)

type extractorServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *extractorServer) SayHello(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	// Check for x-user-id header (mirroring the Rust extractor behavior)
	userID := req.Header().Get("x-user-id")
	if userID == "" {
		return nil, connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("missing x-user-id header"))
	}

	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	msg := fmt.Sprintf("Hello, %s! (authenticated as %s)", name, userID)
	return connect.NewResponse(&gen.HelloResponse{
		Message: msg,
	}), nil
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewHelloWorldServiceHandler(&extractorServer{})
	mux.Handle(path, handler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
