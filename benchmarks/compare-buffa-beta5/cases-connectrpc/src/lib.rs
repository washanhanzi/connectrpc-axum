include!(concat!(env!("OUT_DIR"), "/protos.rs"));

use crate::hello::{
    HelloRequest, HelloRequestView, HelloResponse, HelloWorldService, HelloWorldServiceServer,
    ResponseDetails, hello_request, hello_response,
};
use axum::{Router, body::Body};
use buffa::Message;
use buffa::view::OwnedView;
use bytes::Bytes;
use connectrpc::{ConnectError, ConnectRpcService, Context};
use futures::stream;
use http::Request;
use std::pin::Pin;

#[derive(Clone, Copy, Debug)]
pub enum PayloadSize {
    Small,
    Medium,
    Large,
}

impl PayloadSize {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

fn repeated(label: &str, count: usize) -> String {
    label.repeat(count)
}

pub fn hello_request(size: PayloadSize) -> HelloRequest {
    match size {
        PayloadSize::Small => HelloRequest {
            name: Some("Alice".to_string()),
            hobbies: vec!["cycling".to_string(), "music".to_string()],
            greeting_type: Some(hello_request::GreetingType::FormalGreeting(
                "Hello Alice".to_string(),
            )),
            ..Default::default()
        },
        PayloadSize::Medium => HelloRequest {
            name: Some(repeated("medium-name-", 16)),
            hobbies: (0..64)
                .map(|index| format!("medium-hobby-{index:03}-{}", repeated("x", 24)))
                .collect(),
            greeting_type: Some(hello_request::GreetingType::CasualGreeting(repeated(
                "hello-medium-",
                32,
            ))),
            ..Default::default()
        },
        PayloadSize::Large => HelloRequest {
            name: Some(repeated("large-name-", 256)),
            hobbies: (0..1024)
                .map(|index| format!("large-hobby-{index:04}-{}", repeated("payload-", 8)))
                .collect(),
            greeting_type: Some(hello_request::GreetingType::FormalGreeting(repeated(
                "formal-large-",
                512,
            ))),
            ..Default::default()
        },
    }
}

pub fn encode_hello_request_proto(request: &HelloRequest) -> Vec<u8> {
    request.encode_to_vec()
}

pub fn decode_hello_request_proto(bytes: &[u8]) -> HelloRequest {
    HelloRequest::decode_from_slice(bytes).expect("decode HelloRequest")
}

pub fn encode_hello_request_json(request: &HelloRequest) -> Vec<u8> {
    serde_json::to_vec(request).expect("encode HelloRequest json")
}

pub fn decode_hello_request_json(bytes: &[u8]) -> HelloRequest {
    serde_json::from_slice(bytes).expect("decode HelloRequest json")
}

pub fn decode_hello_response_proto(bytes: &[u8]) -> HelloResponse {
    HelloResponse::decode_from_slice(bytes).expect("decode HelloResponse")
}

pub fn decode_hello_response_json(bytes: &[u8]) -> HelloResponse {
    serde_json::from_slice(bytes).expect("decode HelloResponse json")
}

pub fn envelope_frame(payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(5 + payload.len());
    framed.push(0x00);
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(payload);
    framed
}

pub fn parse_streaming_proto_responses(bytes: &[u8]) -> usize {
    let mut cursor = bytes;
    let mut count = 0;

    while cursor.len() >= 5 {
        let flags = cursor[0];
        let len = u32::from_be_bytes([cursor[1], cursor[2], cursor[3], cursor[4]]) as usize;
        cursor = &cursor[5..];

        if cursor.len() < len {
            break;
        }

        let payload = &cursor[..len];
        cursor = &cursor[len..];

        if flags & 0x02 != 0 {
            break;
        }

        let _ = HelloResponse::decode_from_slice(payload).expect("decode stream frame");
        count += 1;
    }

    count
}

pub fn unary_proto_request(body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/proto")
        .header("Connect-Protocol-Version", "1")
        .body(Body::from(body))
        .expect("build unary proto request")
}

pub fn unary_json_request(body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHello")
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .body(Body::from(body))
        .expect("build unary json request")
}

pub fn stream_proto_request(body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/hello.HelloWorldService/SayHelloStream")
        .header("Content-Type", "application/connect+proto")
        .header("Connect-Protocol-Version", "1")
        .body(Body::from(envelope_frame(&body)))
        .expect("build stream proto request")
}

struct BenchmarkHelloWorldService;

impl HelloWorldService for BenchmarkHelloWorldService {
    async fn say_hello(
        &self,
        ctx: Context,
        request: OwnedView<HelloRequestView<'static>>,
    ) -> Result<(HelloResponse, Context), ConnectError> {
        let name = request.name.unwrap_or("World");

        Ok((
            HelloResponse {
                message: format!("Hello, {name}!"),
                response_type: Some(hello_response::ResponseType::Details(Box::new(
                    ResponseDetails {
                        timestamp: "2026-03-26T00:00:00Z".to_string(),
                        request_count: request.hobbies.len() as i32,
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            },
            ctx,
        ))
    }

    async fn say_hello_stream(
        &self,
        ctx: Context,
        request: OwnedView<HelloRequestView<'static>>,
    ) -> Result<
        (
            Pin<Box<dyn futures::Stream<Item = Result<HelloResponse, ConnectError>> + Send>>,
            Context,
        ),
        ConnectError,
    > {
        let name = request.name.unwrap_or("World");
        let hobby_count = request.hobbies.len() as i32;

        let responses = vec![
            Ok(HelloResponse {
                message: format!("Hello, {name}!"),
                response_type: Some(hello_response::ResponseType::SimpleText(
                    "first".to_string(),
                )),
                ..Default::default()
            }),
            Ok(HelloResponse {
                message: format!("Still streaming, {name}!"),
                response_type: Some(hello_response::ResponseType::Details(Box::new(
                    ResponseDetails {
                        timestamp: "2026-03-26T00:00:01Z".to_string(),
                        request_count: hobby_count,
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            }),
            Ok(HelloResponse {
                message: format!("Goodbye, {name}!"),
                response_type: Some(hello_response::ResponseType::SimpleText("last".to_string())),
                ..Default::default()
            }),
        ];

        Ok((Box::pin(stream::iter(responses)), ctx))
    }

    async fn get_greeting(
        &self,
        ctx: Context,
        request: OwnedView<HelloRequestView<'static>>,
    ) -> Result<(HelloResponse, Context), ConnectError> {
        self.say_hello(ctx, request).await
    }
}

pub fn connect_app() -> Router {
    let service = HelloWorldServiceServer::new(BenchmarkHelloWorldService);
    Router::new().fallback_service(ConnectRpcService::new(service))
}

pub fn response_bytes_to_vec(bytes: Bytes) -> Vec<u8> {
    bytes.to_vec()
}
