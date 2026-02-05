package main

import (
	"context"
	"crypto/tls"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"strings"

	"github.com/connectrpc-axum/test/go/gen"
	"github.com/connectrpc-axum/test/go/gen/genconnect"
	"golang.org/x/net/http2"
)

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	// Use HTTP/2 (h2c) transport since connect-go requires HTTP/2 for bidi streams
	transport := &http2.Transport{
		AllowHTTP: true,
		DialTLSContext: func(ctx context.Context, network, addr string, _ *tls.Config) (net.Conn, error) {
			return net.Dial("unix", socketPath)
		},
	}
	httpClient := &http.Client{Transport: transport}

	failed := false

	if err := testBidiStream(httpClient); err != nil {
		fmt.Printf("    FAIL  bidi stream echoes messages: %v\n", err)
		failed = true
	} else {
		fmt.Printf("    PASS  bidi stream echoes messages\n")
	}

	if failed {
		os.Exit(1)
	}
}

func testBidiStream(client *http.Client) error {
	echoClient := genconnect.NewEchoServiceClient(client, "http://localhost")

	stream := echoClient.EchoBidiStream(context.Background())

	// Send 3 messages
	messages := []string{"Hello", "World", "Bidi"}
	for _, msg := range messages {
		if err := stream.Send(&gen.EchoRequest{Message: msg}); err != nil {
			return fmt.Errorf("sending message %q: %w", msg, err)
		}
	}

	if err := stream.CloseRequest(); err != nil {
		return fmt.Errorf("closing request: %w", err)
	}

	// Receive responses
	var responses []string
	for {
		resp, err := stream.Receive()
		if err != nil {
			break
		}
		responses = append(responses, resp.Message)
	}

	if len(responses) < 3 {
		return fmt.Errorf("expected at least 3 responses, got %d: %v", len(responses), responses)
	}

	if !strings.Contains(responses[0], "Echo #1") {
		return fmt.Errorf("expected first response to contain 'Echo #1', got: %s", responses[0])
	}

	return nil
}
