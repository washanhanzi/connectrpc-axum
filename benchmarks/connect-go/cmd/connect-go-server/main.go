package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"sort"
	"strconv"

	"connectrpc.com/connect"
	"github.com/redis/go-redis/v9"

	fortunev1 "github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/fortune/v1"
	"github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/fortune/v1/fortunev1connect"
)

const (
	fortuneKey   = "fortunes"
	extraFortune = "Additional fortune added at request time."
)

type fortune struct {
	ID      int
	Message string
}

type fortuneHandler struct {
	fortunev1connect.UnimplementedFortuneServiceHandler
	rdb *redis.Client
}

func (h *fortuneHandler) GetFortunes(
	ctx context.Context,
	_ *connect.Request[fortunev1.GetFortunesRequest],
) (*connect.Response[fortunev1.GetFortunesResponse], error) {
	fortunes, err := queryFortunes(ctx, h.rdb)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, err)
	}

	response := &fortunev1.GetFortunesResponse{
		Fortunes: make([]*fortunev1.Fortune, len(fortunes)),
	}
	for i, entry := range fortunes {
		response.Fortunes[i] = &fortunev1.Fortune{
			Id:      int32(entry.ID),
			Message: entry.Message,
		}
	}

	return connect.NewResponse(response), nil
}

func queryFortunes(ctx context.Context, rdb *redis.Client) ([]fortune, error) {
	raw, err := rdb.HGetAll(ctx, fortuneKey).Result()
	if err != nil {
		return nil, err
	}

	fortunes := make([]fortune, 0, len(raw)+1)
	for idString, message := range raw {
		id, _ := strconv.Atoi(idString)
		fortunes = append(fortunes, fortune{ID: id, Message: message})
	}
	fortunes = append(fortunes, fortune{ID: 0, Message: extraFortune})
	sort.Slice(fortunes, func(i, j int) bool {
		return fortunes[i].Message < fortunes[j].Message
	})
	return fortunes, nil
}

func main() {
	if len(os.Args) < 2 {
		panic("usage: connect-go-server <valkey_addr>")
	}

	rdb := redis.NewClient(&redis.Options{Addr: os.Args[1]})
	mux := http.NewServeMux()
	mux.Handle(fortunev1connect.NewFortuneServiceHandler(&fortuneHandler{rdb: rdb}))

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
