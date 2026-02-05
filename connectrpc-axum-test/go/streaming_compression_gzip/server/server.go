package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"strings"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/test/go/gen"
	"github.com/connectrpc-axum/test/go/gen/genconnect"
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

	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Hi %s!", name)}); err != nil {
		return err
	}

	large := fmt.Sprintf("Hello %s! %s %s %s",
		name,
		strings.Repeat("padding_text ", 10),
		strings.Repeat("more_padding ", 10),
		strings.Repeat("final_padding ", 10))
	if err := stream.Send(&gen.HelloResponse{Message: large}); err != nil {
		return err
	}

	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Stream for %s: %s %s",
		name,
		strings.Repeat("repeated_content ", 15),
		strings.Repeat("more_content ", 15))}); err != nil {
		return err
	}

	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Bye %s!", name)}); err != nil {
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

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
