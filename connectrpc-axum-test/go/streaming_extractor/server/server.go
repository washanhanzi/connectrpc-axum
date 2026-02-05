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

func (s *extractorServer) SayHelloStream(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
	stream *connect.ServerStream[gen.HelloResponse],
) error {
	// Check for x-api-key header
	apiKey := req.Header().Get("x-api-key")
	if apiKey == "" {
		return connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("missing api key"))
	}

	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Hello, %s!", name)}); err != nil {
		return err
	}
	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("How are you, %s?", name)}); err != nil {
		return err
	}
	return nil
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
