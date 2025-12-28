package main

import (
	"context"
	"net/http"
	"strings"
	"testing"

	"connectrpc.com/connect"
	"github.com/connectrpc-axum/examples/go-client/gen"
	"github.com/connectrpc-axum/examples/go-client/gen/genconnect"
)

// TestProtocolVersion verifies Connect-Protocol-Version header validation.
//
// The server requires Connect-Protocol-Version: 1 header.
// The connect-go library automatically sends this header.
func TestProtocolVersion(t *testing.T) {
	s := startServer(t, "protocol-version", "")
	defer s.stop()

	client := genconnect.NewHelloWorldServiceClient(
		http.DefaultClient,
		serverURL,
	)

	name := "Protocol Version Tester"
	resp, err := client.SayHello(context.Background(), connect.NewRequest(&gen.HelloRequest{
		Name: &name,
	}))
	if err != nil {
		t.Fatalf("Connect client failed: %v", err)
	}

	if resp.Msg.Message == "" {
		t.Fatal("Empty response message")
	}
	if !strings.Contains(resp.Msg.Message, name) {
		t.Fatalf("Response doesn't contain name: got %q", resp.Msg.Message)
	}

	t.Logf("Response: %s", resp.Msg.Message)
}
