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
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

type streamServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *streamServer) SayHelloStream(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
	stream *connect.ServerStream[gen.HelloResponse],
) error {
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
	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Goodbye, %s!", name)}); err != nil {
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
	path, handler := genconnect.NewHelloWorldServiceHandler(&streamServer{})
	mux.Handle(path, handler)

	h2cHandler := h2c.NewHandler(mux, &http2.Server{})

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	server := &http.Server{Handler: h2cHandler}
	if err := server.Serve(listener); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
