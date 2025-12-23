    - Should Connect proto responses be unframed (application/proto / application/
    connect+proto) rather than gRPC-framed? If yes, the current response encoding is
    non‑spec.
  - Is application/connect+json intended to be accepted for unary requests without a
    frame? If not, frame handling should be strict.


  Answer to Q1 (Connect proto responses):

  - Connect unary responses use application/<codec> (so application/proto or
    application/json) and are unframed. See connectUnaryContentTypePrefix and
    connectContentTypeForCodecName in /tmp/connect-go/protocol_connect.go, plus
    connectUnaryMarshaler.write writing raw bytes without an envelope in /tmp/connect-
    go/protocol_connect.go.
  - application/connect+proto is for streaming responses only (Connect envelope +
    EndStream). See connectStreamingContentTypePrefix and connectStreamingMarshaler in /
    tmp/connect-go/protocol_connect.go, plus response content-type validation in /tmp/
    connect-go/protocol_connect_test.go.

  Answer to Q2 (connect+json unary framing):

  - application/connect+json is not used for unary. Unary JSON uses application/json
    (unframed). Connect streaming uses application/connect+json with envelope framing
    and EndStream. See connectUnaryContentTypePrefix vs
    connectStreamingContentTypePrefix and the streaming envelope/EndStream
    implementation in /tmp/connect-go/protocol_connect.go.