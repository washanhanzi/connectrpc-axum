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

type endstreamMetadataServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *endstreamMetadataServer) SayHelloStream(
	_ context.Context,
	_ *connect.Request[gen.HelloRequest],
	_ *connect.ServerStream[gen.HelloResponse],
) error {
	err := connect.NewError(connect.CodeInternal, fmt.Errorf("something went wrong"))
	err.Meta().Set("x-custom-meta", "custom-value")
	err.Meta().Set("x-request-id", "req-123")
	return err
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewHelloWorldServiceHandler(&endstreamMetadataServer{})
	mux.Handle(path, handler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
