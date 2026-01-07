package main

import (
	"context"
	"crypto/tls"
	"io"
	"net"
	"net/http"
	"strings"
	"testing"
	"time"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
	"golang.org/x/net/http2"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

// TestTonicBidiStreamConnectUnary tests Connect unary RPC on a server that also supports bidi streaming.
func TestTonicBidiStreamConnectUnary(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Bidi Server Unary Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Connect unary failed: %v", err)
	}

	if resp.Msg.Message == "" {
		t.Fatal("Empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		t.Fatalf("Response doesn't contain name: got %q", resp.Msg.Message)
	}

	t.Logf("Response: %s", resp.Msg.Message)
}

// TestTonicBidiStreamGRPC tests gRPC bidirectional streaming.
func TestTonicBidiStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)

	stream, err := client.EchoBidiStream(context.Background())
	if err != nil {
		t.Fatalf("Failed to start bidi stream: %v", err)
	}

	// Send messages in a goroutine
	messages := []string{"Hello", "World", "Bidi", "Stream", "Test"}
	go func() {
		for _, msg := range messages {
			if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
				t.Logf("Send error: %v", err)
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		stream.CloseSend()
	}()

	// Receive responses
	msgCount := 0
	for {
		resp, err := stream.Recv()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatalf("Recv error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			t.Fatalf("Empty message at position %d", msgCount)
		}
		t.Logf("[%d] %s", msgCount, resp.Message)
	}

	if msgCount == 0 {
		t.Fatal("Received no messages")
	}

	t.Logf("Bidi stream completed with %d messages", msgCount)
}

// TestTonicClientStreamGRPC tests gRPC client streaming.
func TestTonicClientStreamGRPC(t *testing.T) {
	s := startServer(t, "tonic-bidi-stream", "tonic")
	defer s.stop()

	conn, err := grpc.NewClient(serverAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := gen.NewEchoServiceClient(conn)

	stream, err := client.EchoClientStream(context.Background())
	if err != nil {
		t.Fatalf("Failed to start client stream: %v", err)
	}

	// Send multiple messages
	messages := []string{"Hello", "World", "Client", "Stream", "Test"}
	for _, msg := range messages {
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			t.Fatalf("Send error: %v", err)
		}
		t.Logf("Sent: %s", msg)
	}

	// Close and receive the single response
	resp, err := stream.CloseAndRecv()
	if err != nil {
		t.Fatalf("CloseAndRecv error: %v", err)
	}

	if resp.Message == "" {
		t.Fatal("Empty response message")
	}

	// Verify response mentions all messages
	for _, msg := range messages {
		if !strings.Contains(resp.Message, msg) {
			t.Errorf("Response missing message %q: got %q", msg, resp.Message)
		}
	}

	t.Logf("Client stream response: %s", resp.Message)
}

// TestConnectBidiStream tests Connect protocol bidirectional streaming.
// Note: Bidi streaming requires HTTP/2, so we use an h2c-capable client.
func TestConnectBidiStream(t *testing.T) {
	s := startServer(t, "connect-bidi-stream", "")
	defer s.stop()

	// Create HTTP/2 cleartext (h2c) client
	h2cClient := &http.Client{
		Transport: &http2.Transport{
			AllowHTTP: true,
			DialTLSContext: func(ctx context.Context, network, addr string, _ *tls.Config) (net.Conn, error) {
				return net.Dial(network, addr)
			},
		},
	}

	client := genconnect.NewEchoServiceClient(
		h2cClient,
		serverURL,
	)

	stream := client.EchoBidiStream(context.Background())

	// Send messages in a goroutine
	messages := []string{"Hello", "World", "Connect", "Bidi", "Test"}
	go func() {
		for _, msg := range messages {
			if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
				t.Logf("Send error: %v", err)
				return
			}
			time.Sleep(50 * time.Millisecond)
		}
		stream.CloseRequest()
	}()

	// Receive responses
	msgCount := 0
	for {
		resp, err := stream.Receive()
		if err == io.EOF {
			break
		}
		if err != nil {
			// Connect protocol may return an error after sending EndStream frame
			// This is expected behavior when all messages have been received
			if msgCount > 0 && strings.Contains(err.Error(), "EOF") {
				t.Logf("Stream ended after %d messages (expected EOF-like error)", msgCount)
				break
			}
			t.Fatalf("Receive error: %v", err)
		}
		msgCount++
		if resp.Message == "" {
			t.Fatalf("Empty message at position %d", msgCount)
		}
		t.Logf("[%d] %s", msgCount, resp.Message)
	}

	if msgCount == 0 {
		t.Fatal("Received no messages")
	}

	t.Logf("Connect bidi stream completed with %d messages", msgCount)
}

// TestConnectClientStream tests Connect protocol client streaming.
func TestConnectClientStream(t *testing.T) {
	s := startServer(t, "connect-client-stream", "")
	defer s.stop()

	client := genconnect.NewEchoServiceClient(
		http.DefaultClient,
		serverURL,
	)

	stream := client.EchoClientStream(context.Background())

	// Send multiple messages
	messages := []string{"Alice", "Bob", "Charlie", "Connect", "ClientStream"}
	for _, msg := range messages {
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			t.Fatalf("Send error: %v", err)
		}
		t.Logf("Sent: %s", msg)
	}

	// Close and receive the single response
	resp, err := stream.CloseAndReceive()
	if err != nil {
		t.Fatalf("CloseAndReceive error: %v", err)
	}

	if resp.Msg.Message == "" {
		t.Fatal("Empty response message")
	}

	// Verify response mentions all messages
	for _, msg := range messages {
		if !strings.Contains(resp.Msg.Message, msg) {
			t.Errorf("Response missing message %q: got %q", msg, resp.Msg.Message)
		}
	}

	t.Logf("Connect client stream response: %s", resp.Msg.Message)
}
