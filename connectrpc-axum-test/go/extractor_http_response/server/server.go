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

type extractorServer struct {
	genconnect.UnimplementedHelloWorldServiceHandler
}

func (s *extractorServer) SayHello(
	_ context.Context,
	req *connect.Request[gen.HelloRequest],
) (*connect.Response[gen.HelloResponse], error) {
	// The middleware already validated x-user-id, so we can safely read it
	userID := req.Header().Get("x-user-id")
	name := "World"
	if req.Msg.Name != nil && *req.Msg.Name != "" {
		name = *req.Msg.Name
	}

	msg := fmt.Sprintf("Hello, %s! (authenticated as %s)", name, userID)
	return connect.NewResponse(&gen.HelloResponse{
		Message: msg,
	}), nil
}

// requireUserID is middleware that returns HTTP 401 when x-user-id header is missing.
func requireUserID(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("x-user-id") == "" {
			w.WriteHeader(http.StatusUnauthorized)
			w.Write([]byte("Unauthorized: missing x-user-id header"))
			return
		}
		next.ServeHTTP(w, r)
	})
}

func main() {
	socketPath := os.Getenv("SOCKET_PATH")
	if socketPath == "" {
		log.Fatal("SOCKET_PATH env var is required")
	}

	mux := http.NewServeMux()
	path, handler := genconnect.NewHelloWorldServiceHandler(&extractorServer{})
	mux.Handle(path, requireUserID(handler))

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("failed to listen on %s: %v", socketPath, err)
	}

	if err := http.Serve(listener, mux); err != nil {
		log.Fatalf("server error: %v", err)
	}
}
