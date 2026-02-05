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
	"google.golang.org/protobuf/types/known/wrapperspb"
)

type errorDetailsServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *errorDetailsServer) SayHello(
	_ context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	// Always return an error with details for this test server.
	err := connect.NewError(connect.CodeInvalidArgument, fmt.Errorf("name is required"))
	detail, detailErr := connect.NewErrorDetail(&wrapperspb.StringValue{Value: "provide a name"})
	if detailErr != nil {
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("failed to create error detail: %w", detailErr))
	}
	err.AddDetail(detail)
	return nil, err
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewHelloWorldServiceHandler(&errorDetailsServer{})
	mux.Handle(path, handler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
