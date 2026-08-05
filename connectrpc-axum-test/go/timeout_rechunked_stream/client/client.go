package main

import (
	"context"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"time"

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
	httpClient := &http.Client{Transport: transport}
	// Unregister gzip so the server does not compress envelopes: per-envelope
	// compression would change the byte layout and hide the decoy bytes the
	// scenario depends on.
	client := genconnect.NewHelloWorldServiceClient(httpClient, "http://localhost",
		connect.WithAcceptCompression("gzip", nil, nil))

	if err := runTest(client); err != nil {
		fmt.Printf("    FAIL  re-chunked stream with client deadline: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    PASS  re-chunked stream with client deadline\n")
}

func runTest(client genconnect.HelloWorldServiceClient) error {
	// The context deadline makes connect-go send Connect-Timeout-Ms, arming
	// the server's deadline body wrapper. The server re-chunks the response
	// body so envelopes span chunks; the full stream must still arrive.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	name := "Rechunk"
	stream, err := client.SayHelloStream(ctx, connect.NewRequest(&gen.HelloRequest{Name: &name}))
	if err != nil {
		return fmt.Errorf("starting stream: %w", err)
	}
	defer stream.Close()

	var messages []string
	for stream.Receive() {
		messages = append(messages, stream.Msg().Message)
	}
	if err := stream.Err(); err != nil {
		return fmt.Errorf("stream failed after %d messages: %w", len(messages), err)
	}

	// First message carries decoy bytes that mimic an EndStream envelope
	// header when they land at the start of a re-chunked body chunk.
	decoy := "A\x02\x00\x00\x00\x00rechunk-decoy"
	expected := []string{decoy, "second for Rechunk", "third for Rechunk"}
	if len(messages) != len(expected) {
		return fmt.Errorf("expected %d messages, got %d: %q", len(expected), len(messages), messages)
	}
	for i := range expected {
		if messages[i] != expected[i] {
			return fmt.Errorf("message %d: expected %q, got %q", i, expected[i], messages[i])
		}
	}
	return nil
}
