package main

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"encoding/json"
	"io"
	"net/http"
	"testing"
)

// compressGzip compresses data using gzip
func compressGzip(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	writer := gzip.NewWriter(&buf)
	if _, err := writer.Write(data); err != nil {
		return nil, err
	}
	if err := writer.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// buildFrame builds a Connect streaming frame
// flags: 0x00 = uncompressed, 0x01 = compressed, 0x02 = end stream
func buildFrame(flags byte, payload []byte) []byte {
	frame := make([]byte, 5+len(payload))
	frame[0] = flags
	binary.BigEndian.PutUint32(frame[1:5], uint32(len(payload)))
	copy(frame[5:], payload)
	return frame
}

// TestClientStreamingCompression tests that per-message decompression works correctly.
// It verifies:
// 1. Compressed frames (flag 0x01) are correctly decompressed by the server
// 2. Uncompressed frames (flag 0x00) are handled normally
// 3. Server can process mixed compressed/uncompressed frames
func TestClientStreamingCompression(t *testing.T) {
	s := startServer(t, "client-streaming-compression", "")
	defer s.stop()

	// Build request body with mixed compressed and uncompressed frames

	// Frame 1: Uncompressed
	payload1 := []byte(`{"message":"Hello from Alice"}`)
	frame1 := buildFrame(0x00, payload1)

	// Frame 2: Compressed - message that will be decompressed by server
	payload2 := []byte(`{"message":"Hello from Bob via compressed frame"}`)
	compressed2, err := compressGzip(payload2)
	if err != nil {
		t.Fatalf("Failed to compress payload2: %v", err)
	}
	frame2 := buildFrame(0x01, compressed2)

	// Frame 3: Compressed
	payload3 := []byte(`{"message":"Hello from Charlie also compressed"}`)
	compressed3, err := compressGzip(payload3)
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
	t.Logf("  Frame 2: compressed, %d bytes -> %d bytes", len(payload2), len(compressed2))
	t.Logf("  Frame 3: compressed, %d bytes -> %d bytes", len(payload3), len(compressed3))

	req, err := http.NewRequest(
		"POST",
		serverURL+"/echo.EchoService/EchoClientStream",
		&reqBody,
	)
	if err != nil {
		t.Fatalf("Failed to create request: %v", err)
	}

	// Set headers for Connect streaming with gzip compression
	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Content-Encoding", "gzip") // Tell server frames use gzip

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
						payload, err = decompressGzip(frame.Payload)
						if err != nil {
							t.Errorf("Failed to decompress response frame %d: %v", i, err)
							continue
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
						if !bytes.Contains([]byte(respMsg.Message), []byte("Alice")) {
							t.Errorf("Response missing 'Alice'")
						}
						if !bytes.Contains([]byte(respMsg.Message), []byte("Bob")) {
							t.Errorf("Response missing 'Bob' (from compressed frame)")
						}
						if !bytes.Contains([]byte(respMsg.Message), []byte("Charlie")) {
							t.Errorf("Response missing 'Charlie' (from compressed frame)")
						}
						if !bytes.Contains([]byte(respMsg.Message), []byte("3 messages")) {
							t.Errorf("Response should mention receiving 3 messages")
						}
					}
				}
			}

			t.Logf("✓ Client streaming compression test passed: server correctly decompressed frames")
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

	// Verify the response contains all three messages
	if !bytes.Contains([]byte(respMsg.Message), []byte("Alice")) {
		t.Errorf("Response missing 'Alice'")
	}
	if !bytes.Contains([]byte(respMsg.Message), []byte("Bob")) {
		t.Errorf("Response missing 'Bob' (from compressed frame)")
	}
	if !bytes.Contains([]byte(respMsg.Message), []byte("Charlie")) {
		t.Errorf("Response missing 'Charlie' (from compressed frame)")
	}
	if !bytes.Contains([]byte(respMsg.Message), []byte("3 messages")) {
		t.Errorf("Response should mention receiving 3 messages")
	}

	t.Logf("✓ Client streaming compression test passed: server correctly decompressed frames")
}
