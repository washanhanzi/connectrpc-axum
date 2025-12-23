module github.com/connectrpc-axum/examples/go-client

go 1.24.0

toolchain go1.24.3

require (
	connectrpc.com/connect v1.16.2
	github.com/connectrpc-axum/examples/go-client/gen v0.0.0
	github.com/connectrpc-axum/examples/go-client/gen/genconnect v0.0.0
	golang.org/x/net v0.46.0
	google.golang.org/grpc v1.68.0
	google.golang.org/protobuf v1.34.2
)

require (
	golang.org/x/sys v0.37.0 // indirect
	golang.org/x/text v0.30.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20240903143218-8af14fe29dc1 // indirect
)

replace (
	github.com/connectrpc-axum/examples/go-client/gen => ./gen
	github.com/connectrpc-axum/examples/go-client/gen/genconnect => ./gen/genconnect
)
