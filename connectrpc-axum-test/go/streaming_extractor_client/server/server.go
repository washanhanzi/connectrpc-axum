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

type echoServer struct {
	genconnect.UnimplementedEchoServiceHandler
}

func (s *echoServer) EchoClientStream(
	ctx context.Context,
	stream *connect.ClientStream[gen.EchoRequest],
) (*connect.Response[gen.EchoResponse], error) {
	apiKey := stream.RequestHeader().Get("x-api-key")
	if apiKey == "" {
		return nil, connect.NewError(connect.CodeUnauthenticated, fmt.Errorf("missing api key"))
	}

	var messages []string
	for stream.Receive() {
		messages = append(messages, stream.Msg().Message)
	}
	if err := stream.Err(); err != nil {
		return nil, err
	}
	return connect.NewResponse(&gen.EchoResponse{
		Message: fmt.Sprintf("Received %d messages from %s: [%s]", len(messages), apiKey, strings.Join(messages, ", ")),
	}), nil
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewEchoServiceHandler(&echoServer{})
	mux.Handle(path, handler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
