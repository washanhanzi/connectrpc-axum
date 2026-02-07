module github.com/connectrpc-axum/test

go 1.24.0

require (
	connectrpc.com/connect v1.16.2
	github.com/connectrpc-axum/test/go/gen v0.0.0
	github.com/connectrpc-axum/test/go/gen/genconnect v0.0.0
	golang.org/x/net v0.38.0
	google.golang.org/grpc v1.68.0
	google.golang.org/protobuf v1.34.2
)

require (
	golang.org/x/sys v0.31.0 // indirect
	golang.org/x/text v0.23.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20240903143218-8af14fe29dc1 // indirect
)

replace (
	github.com/connectrpc-axum/test/go/gen => ./gen
	github.com/connectrpc-axum/test/go/gen/genconnect => ./gen/genconnect
)
