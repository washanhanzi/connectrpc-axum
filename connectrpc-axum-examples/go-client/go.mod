module github.com/connectrpc-axum/examples/go-client

go 1.24.0

toolchain go1.24.3

require (
	connectrpc.com/connect v1.16.2
	github.com/connectrpc-axum/examples/go-client/gen/hellopb v0.0.0
	github.com/connectrpc-axum/examples/go-client/gen/hellopbconnect v0.0.0-00010101000000-000000000000
	golang.org/x/net v0.46.0
	google.golang.org/protobuf v1.34.2
)

require golang.org/x/text v0.30.0 // indirect

replace (
	github.com/connectrpc-axum/examples/go-client/gen/echopb => ./gen/echopb
	github.com/connectrpc-axum/examples/go-client/gen/echopbconnect => ./gen/echopbconnect
	github.com/connectrpc-axum/examples/go-client/gen/hellopb => ./gen/hellopb
	github.com/connectrpc-axum/examples/go-client/gen/hellopbconnect => ./gen/hellopbconnect
)
