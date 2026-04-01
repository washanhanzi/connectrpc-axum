package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"

	"connectrpc.com/connect"

	benchv1 "github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/bench/v1"
	"github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/bench/v1/benchv1connect"
)

type benchHandler struct {
	benchv1connect.UnimplementedBenchServiceHandler
}

func (h *benchHandler) Unary(
	_ context.Context,
	req *connect.Request[benchv1.BenchRequest],
) (*connect.Response[benchv1.BenchResponse], error) {
	return connect.NewResponse(&benchv1.BenchResponse{
		Payload: req.Msg.Payload,
	}), nil
}

func main() {
	if len(os.Args) != 1 {
		panic("usage: connect-go-protocol-server")
	}

	mux := http.NewServeMux()
	mux.Handle(benchv1connect.NewBenchServiceHandler(&benchHandler{}))

	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		panic(err)
	}

	fmt.Println(listener.Addr().String())

	protocols := new(http.Protocols)
	protocols.SetHTTP1(true)
	protocols.SetUnencryptedHTTP2(true)

	server := &http.Server{Handler: mux, Protocols: protocols}
	if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
		panic(err)
	}
}
