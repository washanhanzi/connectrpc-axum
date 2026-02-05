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

type echoServer struct {
	genconnect.UnimplementedEchoServiceHandler
}

func (s *echoServer) EchoBidiStream(
	ctx context.Context,
	stream *connect.BidiStream[gen.EchoRequest, gen.EchoResponse],
) error {
	count := 0
	for {
		msg, err := stream.Receive()
		if err != nil {
			break
		}
		count++
		if err := stream.Send(&gen.EchoResponse{
			Message: fmt.Sprintf("Echo #%d: %s", count, msg.Message),
		}); err != nil {
			return err
		}
	}
	return stream.Send(&gen.EchoResponse{
		Message: fmt.Sprintf("Stream complete. Echoed %d messages.", count),
	})
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewEchoServiceHandler(&echoServer{})
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
