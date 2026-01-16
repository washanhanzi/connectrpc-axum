package main

import (
	"bytes"
	"compress/zlib"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"testing"

	"github.com/andybalholm/brotli"
	"github.com/klauspost/compress/zstd"
)

// Compression/decompression functions for each algorithm

// Deflate (zlib-wrapped per HTTP spec)
func compressDeflate(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	writer := zlib.NewWriter(&buf)
	if _, err := writer.Write(data); err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func decompressDeflate(data []byte) ([]byte, error) {
	reader, err := zlib.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	return io.ReadAll(reader)
}

// Brotli
func compressBrotli(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	writer := brotli.NewWriter(&buf)
	if _, err := writer.Write(data); err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func decompressBrotli(data []byte) ([]byte, error) {
	reader := brotli.NewReader(bytes.NewReader(data))
	return io.ReadAll(reader)
}

// Zstd
func compressZstd(data []byte) ([]byte, error) {
	encoder, err := zstd.NewWriter(nil)
	if err != nil {
		return nil, err
	}
	defer encoder.Close()
	return encoder.EncodeAll(data, nil), nil
}

func decompressZstd(data []byte) ([]byte, error) {
	decoder, err := zstd.NewReader(nil)
	if err != nil {
		return nil, err
	}
	defer decoder.Close()
	return decoder.DecodeAll(data, nil)
}

// Note: compressGzip and decompressGzip are defined in client_streaming_compression_test.go
// and streaming_compression_test.go respectively (same package).

// Decompressor type for polymorphism
type decompressor func([]byte) ([]byte, error)
type compressor func([]byte) ([]byte, error)

// TestStreamingResponseCompression_Deflate tests server->client compression using deflate
func TestStreamingResponseCompression_Deflate(t *testing.T) {
	testStreamingResponseCompression(t, "deflate", decompressDeflate)
}

// TestStreamingResponseCompression_Brotli tests server->client compression using brotli
func TestStreamingResponseCompression_Brotli(t *testing.T) {
	testStreamingResponseCompression(t, "br", decompressBrotli)
}

// TestStreamingResponseCompression_Zstd tests server->client compression using zstd
func TestStreamingResponseCompression_Zstd(t *testing.T) {
	testStreamingResponseCompression(t, "zstd", decompressZstd)
}

// testStreamingResponseCompression is a helper that tests server->client streaming compression
// for a given encoding algorithm
func testStreamingResponseCompression(t *testing.T, encoding string, decompress decompressor) {
	s := startServer(t, "streaming-compression-algos", "compression-full")
	defer s.stop()

	// Build request with Connect streaming format: [flags:1][length:4][payload]
	jsonPayload := []byte(`{"name":"TestUser"}`)
	reqBody := make([]byte, 5+len(jsonPayload))
	reqBody[0] = 0x00 // flags (no compression, not end stream)
	binary.BigEndian.PutUint32(reqBody[1:5], uint32(len(jsonPayload)))
	copy(reqBody[5:], jsonPayload)

	req, err := http.NewRequest(
		"POST",
		serverURL+"/hello.HelloWorldService/SayHelloStream",
		bytes.NewReader(reqBody),
	)
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}

	// Set headers for Connect streaming with specific compression
	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Accept-Encoding", encoding) // Request specific compression

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("Request failed: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		t.Fatalf("Unexpected status %d: %s", resp.StatusCode, string(body))
	}

	// Verify Content-Type is streaming
	contentType := resp.Header.Get("Content-Type")
	if contentType != "application/connect+json" {
		t.Fatalf("Expected Content-Type application/connect+json, got %s", contentType)
	}

	// Verify Connect-Content-Encoding header matches the requested encoding
	connectContentEncoding := resp.Header.Get("Connect-Content-Encoding")
	t.Logf("Connect-Content-Encoding: %q", connectContentEncoding)
	if connectContentEncoding != encoding {
		t.Errorf("Expected Connect-Content-Encoding: %s, got %q", encoding, connectContentEncoding)
		t.Error("Without this header, clients won't know how to decompress frames with flag 0x01")
	}

	// Read raw response body
	rawBody, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("Failed to read response body: %v", err)
	}

	t.Logf("Response body length: %d bytes", len(rawBody))

	// Parse frames
	frames, err := parseFrames(rawBody)
	if err != nil {
		t.Fatalf("Failed to parse frames: %v", err)
	}

	t.Logf("Parsed %d frames", len(frames))

	// We expect at least 3 frames:
	// - 1 small message (uncompressed, flag 0x00)
	// - 2+ large messages (compressed, flag 0x01 if encoding accepted)
	// - 1 EndStream (flag 0x02)
	if len(frames) < 3 {
		t.Fatalf("Expected at least 3 frames, got %d", len(frames))
	}

	var (
		compressedCount   int
		uncompressedCount int
		endStreamCount    int
	)

	for i, frame := range frames {
		switch frame.Flags {
		case flagMessage:
			uncompressedCount++
			t.Logf("Frame %d: UNCOMPRESSED (0x00), payload length=%d", i, len(frame.Payload))

			// Try to parse as JSON to verify it's valid
			var msg map[string]interface{}
			if err := json.Unmarshal(frame.Payload, &msg); err != nil {
				t.Errorf("Frame %d: failed to parse uncompressed payload as JSON: %v", i, err)
			} else {
				t.Logf("  Message: %v", msg["message"])
			}

		case flagCompressed:
			compressedCount++
			t.Logf("Frame %d: COMPRESSED (0x01), compressed length=%d", i, len(frame.Payload))

			// Decompress using the specified algorithm and verify
			decompressed, err := decompress(frame.Payload)
			if err != nil {
				t.Errorf("Frame %d: failed to decompress %s: %v", i, encoding, err)
				continue
			}
			t.Logf("  Decompressed length: %d", len(decompressed))

			var msg map[string]interface{}
			if err := json.Unmarshal(decompressed, &msg); err != nil {
				t.Errorf("Frame %d: failed to parse decompressed payload as JSON: %v", i, err)
			} else {
				// Truncate long messages for logging
				msgStr := fmt.Sprintf("%v", msg["message"])
				if len(msgStr) > 100 {
					msgStr = msgStr[:100] + "..."
				}
				t.Logf("  Message: %s", msgStr)
			}

		case flagEndStream:
			endStreamCount++
			t.Logf("Frame %d: END_STREAM (0x02), payload=%s", i, string(frame.Payload))

		default:
			t.Errorf("Frame %d: unexpected flags 0x%02x", i, frame.Flags)
		}
	}

	// Verify we got the expected frame types
	t.Logf("Summary: %d uncompressed, %d compressed, %d end-stream",
		uncompressedCount, compressedCount, endStreamCount)

	if endStreamCount != 1 {
		t.Errorf("Expected exactly 1 EndStream frame, got %d", endStreamCount)
	}

	// The server sends:
	// - 1 small message ("Hi TestUser!") - should be uncompressed
	// - 2 large messages (with padding) - should be compressed
	// - 1 small message ("Bye TestUser!") - should be uncompressed
	// So we expect at least 1 compressed frame for this to be a valid compression test
	if compressedCount == 0 {
		t.Errorf("Expected at least 1 compressed frame (flag 0x01), got 0")
		t.Errorf("This suggests per-message %s compression is not working!", encoding)
	}

	if uncompressedCount == 0 {
		t.Errorf("Expected at least 1 uncompressed frame (flag 0x00), got 0")
	}

	t.Logf("✓ Streaming %s compression test passed: verified flag 0x01 is set on compressed frames", encoding)
}

// TestClientStreamingDecompression_Deflate tests client->server decompression using deflate
func TestClientStreamingDecompression_Deflate(t *testing.T) {
	testClientStreamingDecompression(t, "deflate", compressDeflate)
}

// TestClientStreamingDecompression_Brotli tests client->server decompression using brotli
func TestClientStreamingDecompression_Brotli(t *testing.T) {
	testClientStreamingDecompression(t, "br", compressBrotli)
}

// TestClientStreamingDecompression_Zstd tests client->server decompression using zstd
func TestClientStreamingDecompression_Zstd(t *testing.T) {
	testClientStreamingDecompression(t, "zstd", compressZstd)
}

// testClientStreamingDecompression tests that server can decompress client-sent frames
func testClientStreamingDecompression(t *testing.T, encoding string, compress compressor) {
	s := startServer(t, "client-streaming-compression-algos", "compression-full")
	defer s.stop()

	// Build request body with mixed compressed and uncompressed frames

	// Frame 1: Uncompressed
	payload1 := []byte(`{"message":"Hello from Alice"}`)
	frame1 := buildFrame(0x00, payload1)

	// Frame 2: Compressed - message that will be decompressed by server
	payload2 := []byte(`{"message":"Hello from Bob via compressed frame"}`)
	compressed2, err := compress(payload2)
	if err != nil {
		t.Fatalf("Failed to compress payload2: %v", err)
	}
	frame2 := buildFrame(0x01, compressed2)

	// Frame 3: Compressed
	payload3 := []byte(`{"message":"Hello from Charlie also compressed"}`)
	compressed3, err := compress(payload3)
	if err != nil {
		t.Fatalf("Failed to compress payload3: %v", err)
	}
	frame3 := buildFrame(0x01, compressed3)

	// Frame 4: EndStream (empty payload)
	endFrame := buildFrame(0x02, []byte("{}"))

	// Combine all frames
	var reqBody bytes.Buffer
	reqBody.Write(frame1)
	reqBody.Write(frame2)
	reqBody.Write(frame3)
	reqBody.Write(endFrame)

	t.Logf("Request body: %d bytes total", reqBody.Len())
	t.Logf("  Frame 1: uncompressed, %d bytes payload", len(payload1))
	t.Logf("  Frame 2: compressed with %s, %d bytes -> %d bytes", encoding, len(payload2), len(compressed2))
	t.Logf("  Frame 3: compressed with %s, %d bytes -> %d bytes", encoding, len(payload3), len(compressed3))

	req, err := http.NewRequest(
		"POST",
		serverURL+"/echo.EchoService/EchoClientStream",
		&reqBody,
	)
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}

	// Set headers for Connect streaming with specific compression
	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Content-Encoding", encoding) // Tell server frames use this encoding

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("Request failed: %v", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("Unexpected status %d: %s", resp.StatusCode, string(body))
	}

	t.Logf("Response status: %d", resp.StatusCode)
	t.Logf("Response body: %s", string(body))

	// Parse response - should be streaming format
	// Try to parse as streaming (with envelope) first
	if len(body) >= 5 {
		frames, err := parseFrames(body)
		if err == nil && len(frames) > 0 {
			t.Logf("Parsed %d response frames", len(frames))

			// Find the message frame (not end stream)
			for i, frame := range frames {
				if frame.Flags == 0x00 || frame.Flags == 0x01 {
					payload := frame.Payload
					if frame.Flags == 0x01 {
						// Response might be compressed with gzip (server default) or same encoding
						// Try gzip first, then the same encoding
						var decompErr error
						payload, decompErr = decompressGzip(frame.Payload)
						if decompErr != nil {
							// Not gzip, might be uncompressed or error
							payload = frame.Payload
						}
					}

					var respMsg struct {
						Message string `json:"message"`
					}
					if err := json.Unmarshal(payload, &respMsg); err != nil {
						t.Errorf("Frame %d: failed to parse response: %v", i, err)
					} else {
						t.Logf("Frame %d response message: %s", i, respMsg.Message)

						// Verify the response contains all three messages
						verifyMessageContent(t, respMsg.Message)
					}
				}
			}

			t.Logf("✓ Client streaming %s decompression test passed: server correctly decompressed frames", encoding)
			return
		}
	}

	// If not streaming format, try parsing as plain JSON (unary response)
	var respMsg struct {
		Message string `json:"message"`
	}
	if err := json.Unmarshal(body, &respMsg); err != nil {
		t.Fatalf("Failed to parse response as JSON: %v (body: %s)", err, string(body))
	}

	t.Logf("Response message: %s", respMsg.Message)
	verifyMessageContent(t, respMsg.Message)
	t.Logf("✓ Client streaming %s decompression test passed: server correctly decompressed frames", encoding)
}

func verifyMessageContent(t *testing.T, message string) {
	if !bytes.Contains([]byte(message), []byte("Alice")) {
		t.Errorf("Response missing 'Alice'")
	}
	if !bytes.Contains([]byte(message), []byte("Bob")) {
		t.Errorf("Response missing 'Bob' (from compressed frame)")
	}
	if !bytes.Contains([]byte(message), []byte("Charlie")) {
		t.Errorf("Response missing 'Charlie' (from compressed frame)")
	}
	if !bytes.Contains([]byte(message), []byte("3 messages")) {
		t.Errorf("Response should mention receiving 3 messages")
	}
}

// TestUnaryCompression_Gzip tests unary request/response compression using gzip.
// This is a critical regression test: gzip is the default compression algorithm
// and must always work. Unlike deflate/br/zstd which require feature flags,
// gzip is always enabled via tower-http's compression-gzip feature.
func TestUnaryCompression_Gzip(t *testing.T) {
	testUnaryCompression(t, "gzip", compressGzip, decompressGzip)
}

// TestUnaryCompression_Deflate tests unary request/response compression using deflate
func TestUnaryCompression_Deflate(t *testing.T) {
	testUnaryCompression(t, "deflate", compressDeflate, decompressDeflate)
}

// TestUnaryCompression_Brotli tests unary request/response compression using brotli
func TestUnaryCompression_Brotli(t *testing.T) {
	testUnaryCompression(t, "br", compressBrotli, decompressBrotli)
}

// TestUnaryCompression_Zstd tests unary request/response compression using zstd
func TestUnaryCompression_Zstd(t *testing.T) {
	testUnaryCompression(t, "zstd", compressZstd, decompressZstd)
}

// testUnaryCompression tests unary RPC compression with standard HTTP headers
func testUnaryCompression(t *testing.T, encoding string, compress compressor, decompress decompressor) {
	s := startServer(t, "unary-compression-algos", "compression-full")
	defer s.stop()

	// Create a large request body to ensure compression is triggered
	// The server has a 100-byte compression threshold
	largeMessage := "Test message with lots of padding to exceed compression threshold: " +
		"padding padding padding padding padding padding padding padding padding padding " +
		"more padding more padding more padding more padding more padding more padding"

	jsonPayload := []byte(fmt.Sprintf(`{"name":"%s"}`, largeMessage))

	// Compress the request body
	compressedPayload, err := compress(jsonPayload)
	if err != nil {
		t.Fatalf("Failed to compress request: %v", err)
	}

	t.Logf("Request: original=%d bytes, compressed=%d bytes", len(jsonPayload), len(compressedPayload))

	req, err := http.NewRequest(
		"POST",
		serverURL+"/hello.HelloWorldService/SayHello",
		bytes.NewReader(compressedPayload),
	)
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}

	// Set headers for unary Connect with compression
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Content-Encoding", encoding) // Tell server the request body is compressed
	req.Header.Set("Accept-Encoding", encoding)  // Request compressed response

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("Request failed: %v", err)
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("Unexpected status %d: %s", resp.StatusCode, string(body))
	}

	t.Logf("Response status: %d", resp.StatusCode)
	t.Logf("Response Content-Encoding: %s", resp.Header.Get("Content-Encoding"))
	t.Logf("Response body length: %d bytes", len(body))

	// Check if response is compressed
	contentEncoding := resp.Header.Get("Content-Encoding")

	var responseBody []byte
	if contentEncoding == encoding {
		t.Logf("Response is compressed with %s", encoding)
		responseBody, err = decompress(body)
		if err != nil {
			t.Fatalf("Failed to decompress response: %v", err)
		}
		t.Logf("Decompressed response: %d bytes", len(responseBody))
	} else if contentEncoding == "" {
		// Response might not be compressed if it's small enough
		t.Logf("Response is not compressed (Content-Encoding header absent)")
		responseBody = body
	} else {
		t.Fatalf("Unexpected Content-Encoding: %s", contentEncoding)
	}

	// Parse response
	var respMsg struct {
		Message string `json:"message"`
	}
	if err := json.Unmarshal(responseBody, &respMsg); err != nil {
		t.Fatalf("Failed to parse response JSON: %v (body: %s)", err, string(responseBody))
	}

	t.Logf("Response message: %s", respMsg.Message)

	// Verify the response contains some reference to our input
	if respMsg.Message == "" {
		t.Errorf("Expected non-empty response message")
	}

	t.Logf("✓ Unary %s compression test passed", encoding)
}
