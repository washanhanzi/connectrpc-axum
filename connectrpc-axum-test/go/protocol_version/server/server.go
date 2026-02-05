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

type protocolVersionServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *protocolVersionServer) SayHello(
	_ context.Context,
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

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewHelloWorldServiceHandler(
		&protocolVersionServer{},
		connect.WithRequireConnectProtocolHeader(),
	)
	mux.Handle(path, handler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
