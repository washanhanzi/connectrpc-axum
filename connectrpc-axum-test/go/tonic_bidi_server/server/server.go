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
	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

type bidiServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
	genconnect.UnimplementedEchoServiceHandler
}

func (s *bidiServer) SayHello(
	ctx context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}
	return connect.NewResponse(&gen.HelloResponse{
		Message: fmt.Sprintf("Hello, %s!", name),
	}), nil
}

func (s *bidiServer) EchoBidiStream(
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
	return nil
}

func (s *bidiServer) EchoClientStream(
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
	return connect.NewResponse(&gen.EchoResponse{
		Message: fmt.Sprintf("Received %d messages: [%s]", len(messages), strings.Join(messages, ", ")),
	}), nil
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()

	svc := &bidiServer{}
	helloPath, helloHandler := genconnect.NewHelloWorldServiceHandler(svc)
	mux.Handle(helloPath, helloHandler)
	echoPath, echoHandler := genconnect.NewEchoServiceHandler(svc)
	mux.Handle(echoPath, echoHandler)

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
