module github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go

go 1.24.0

require (
	connectrpc.com/connect v1.19.1
	github.com/redis/go-redis/v9 v9.18.0
	google.golang.org/protobuf v1.36.11
)

replace connectrpc.com/connect => ../../connect-go

require (
	github.com/cespare/xxhash/v2 v2.3.0 // indirect
	github.com/dgryski/go-rendezvous v0.0.0-20200823014737-9f7001d12a5f // indirect
	go.uber.org/atomic v1.11.0 // indirect
)
