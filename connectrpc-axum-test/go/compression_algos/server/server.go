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

type helloServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *helloServer) SayHello(
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

func (s *helloServer) SayHelloStream(
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
	large := fmt.Sprintf("Hello %s! %s %s %s", name,
		strings.Repeat("padding_text ", 10),
		strings.Repeat("more_padding ", 10),
		strings.Repeat("final_padding ", 10))
	if err := stream.Send(&gen.HelloResponse{Message: large}); err != nil {
		return err
	}
	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Stream for %s: %s %s", name,
		strings.Repeat("repeated_content ", 15),
		strings.Repeat("more_content ", 15))}); err != nil {
		return err
	}
	if err := stream.Send(&gen.HelloResponse{Message: fmt.Sprintf("Bye %s!", name)}); err != nil {
		return err
	}
	return nil
}

type echoServer struct {
	genconnect.UnimplementedEchoServiceHandler
}

func (s *echoServer) EchoClientStream(
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
	helloPath, helloHandler := genconnect.NewHelloWorldServiceHandler(&helloServer{})
	mux.Handle(helloPath, helloHandler)
	echoPath, echoHandler := genconnect.NewEchoServiceHandler(&echoServer{})
	mux.Handle(echoPath, echoHandler)

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
