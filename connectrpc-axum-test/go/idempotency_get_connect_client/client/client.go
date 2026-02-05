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

	// Use connect.WithHTTPGet() so the client automatically sends GET for idempotent methods
	connectClient := genconnect.NewHelloWorldServiceClient(httpClient, "http://localhost", connect.WithHTTPGet())

	name := "GetUser"
	resp, err := connectClient.GetGreeting(context.Background(), connect.NewRequest(&gen.HelloRequest{Name: &name}))
	if err != nil {
		fmt.Printf("    FAIL  connect-go client uses HTTP GET: %v\n", err)
		os.Exit(1)
	}

	if !strings.Contains(resp.Msg.Message, "GetUser") {
		fmt.Printf("    FAIL  connect-go client uses HTTP GET: expected 'GetUser' in response, got: %s\n", resp.Msg.Message)
		os.Exit(1)
	}

	fmt.Printf("    PASS  connect-go client uses HTTP GET\n")
}
