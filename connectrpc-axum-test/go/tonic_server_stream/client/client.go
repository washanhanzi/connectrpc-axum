package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/test/go/gen"
	"github.com/connectrpc-axum/test/go/gen/genconnect"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
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
	httpClient := &http.Client{Transport: transport}

	failed := false

	if err := testConnectStream(httpClient); err != nil {
		fmt.Printf("    FAIL  tonic server stream via Connect protocol: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic server stream via Connect protocol\n")
	}

	if err := testGRPCStream(socketPath); err != nil {
		fmt.Printf("    FAIL  tonic server stream via gRPC protocol: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  tonic server stream via gRPC protocol\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testConnectStream(client *http.Client) error {
	connectClient := genconnect.NewHelloWorldServiceClient(client, "http://localhost")
	name := "Alice"
	stream, err := connectClient.SayHelloStream(context.Background(), connect.NewRequest(&gen.HelloRequest{Name: &name}))
	if err != nil {
		return fmt.Errorf("failed to start stream: %w", err)
	}

	msgCount := 0
	for stream.Receive() {
		msgCount++
		if stream.Msg().Message == "" {
			return fmt.Errorf("empty message at position %d", msgCount)
		}
	}
	if err := stream.Err(); err != nil {
		return fmt.Errorf("stream error: %w", err)
	}
	if msgCount < 2 {
		return fmt.Errorf("expected at least 2 messages, got %d", msgCount)
	}
	return nil
}

func testGRPCStream(socketPath string) error {
	conn, err := grpc.NewClient(
		"passthrough:///unix",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		}),
	)
	if err != nil {
		return fmt.Errorf("failed to connect: %w", err)
	}
	defer conn.Close()

	client := gen.NewHelloWorldServiceClient(conn)
	name := "Bob"
	stream, err := client.SayHelloStream(context.Background(), &gen.HelloRequest{Name: &name})
	if err != nil {
		return fmt.Errorf("failed to start gRPC stream: %w", err)
	}

	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("recv error: %w", err)
		}
		msgCount++
		if resp.Message == "" {
			return fmt.Errorf("empty message at position %d", msgCount)
		}
		if msgCount == 1 && !strings.Contains(resp.Message, "Bob") {
			return fmt.Errorf("expected first message to contain 'Bob', got: %s", resp.Message)
		}
	}

	if msgCount < 2 {
		return fmt.Errorf("expected at least 2 messages, got %d", msgCount)
	}
	return nil
}
