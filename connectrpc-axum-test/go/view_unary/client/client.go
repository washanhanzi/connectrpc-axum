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

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	transport := &http.Transport{
		DialContext: func(_ context.Context, _, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	client := &http.Client{Transport: transport}

	failures := 0
	if err := testProtoUnary(client); err != nil {
		fmt.Printf("    FAIL  view unary via Go client (proto): %v\n", err)
		failures++
	} else {
		fmt.Printf("    PASS  view unary via Go client (proto)\n")
	}

	if err := testJSONUnary(client); err != nil {
		fmt.Printf("    FAIL  view unary via Go client (json): %v\n", err)
		failures++
	} else {
		fmt.Printf("    PASS  view unary via Go client (json)\n")
	}

	if failures > 0 {
		os.Exit(1)
	}
}

func testProtoUnary(client *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(client, "http://localhost")
	name := "Proto View"
	resp, err := connectClient.SayHello(
		context.Background(),
		connect.NewRequest(&gen.HelloRequest{Name: &name}),
	)
	if err != nil {
		return fmt.Errorf("Connect proto unary failed: %w", err)
	}
	if resp.Msg.Message != "Hello, Proto View!" {
		return fmt.Errorf("unexpected response: %q", resp.Msg.Message)
	}
	return nil
}

func testJSONUnary(client *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(
		client,
		"http://localhost",
		connect.WithProtoJSON(),
	)
	name := "JSON View"
	resp, err := connectClient.SayHello(
		context.Background(),
		connect.NewRequest(&gen.HelloRequest{Name: &name}),
	)
	if err != nil {
		return fmt.Errorf("Connect JSON unary failed: %w", err)
	}
	if resp.Msg.Message != "Hello, JSON View!" {
		return fmt.Errorf("unexpected response: %q", resp.Msg.Message)
	}
	return nil
}
