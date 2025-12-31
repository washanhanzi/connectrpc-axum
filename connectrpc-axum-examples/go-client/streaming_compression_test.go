package main

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"testing"
)

// Connect streaming frame flags
const (
	flagMessage    = 0x00 // Regular uncompressed message
	flagCompressed = 0x01 // Compressed message
	flagEndStream  = 0x02 // End of stream
)

// Frame represents a Connect streaming frame
type Frame struct {
	Flags   uint8
	Payload []byte
}

// parseFrames parses Connect streaming frames from raw bytes
func parseFrames(data []byte) ([]Frame, error) {
	var frames []Frame
	reader := bytes.NewReader(data)

	for reader.Len() > 0 {
		// Read 5-byte header: [flags:1][length:4]
		var flags uint8
		if err := binary.Read(reader, binary.BigEndian, &flags); err != nil {
			if err == io.EOF {
				break
			}
			return nil, fmt.Errorf("failed to read flags: %w", err)
		}

		var length uint32
		if err := binary.Read(reader, binary.BigEndian, &length); err != nil {
			return nil, fmt.Errorf("failed to read length: %w", err)
		}

		// Read payload
		payload := make([]byte, length)
		if _, err := io.ReadFull(reader, payload); err != nil {
			return nil, fmt.Errorf("failed to read payload: %w", err)
		}

		frames = append(frames, Frame{
			Flags:   flags,
			Payload: payload,
		})
	}

	return frames, nil
}

// decompressGzip decompresses gzip data
func decompressGzip(data []byte) ([]byte, error) {
	reader, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	defer reader.Close()
	return io.ReadAll(reader)
}

// TestStreamingCompression tests that per-message compression works correctly.
// It verifies:
// 1. Small messages have flag 0x00 (uncompressed)
// 2. Large messages have flag 0x01 (compressed) when client accepts gzip
// 3. EndStream frame has flag 0x02
func TestStreamingCompression(t *testing.T) {
	s := startServer(t, "streaming-compression", "")
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

	// Set headers for Connect streaming with gzip compression
	req.Header.Set("Content-Type", "application/connect+json")
	req.Header.Set("Connect-Protocol-Version", "1")
	req.Header.Set("Connect-Accept-Encoding", "gzip") // Request compressed responses

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

	// Verify Connect-Content-Encoding header is set when compression is negotiated
	// This is REQUIRED by the Connect protocol - clients use this header to know
	// how to decompress frames with flag 0x01
	connectContentEncoding := resp.Header.Get("Connect-Content-Encoding")
	t.Logf("Connect-Content-Encoding: %q", connectContentEncoding)
	if connectContentEncoding != "gzip" {
		t.Errorf("Expected Connect-Content-Encoding: gzip, got %q", connectContentEncoding)
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
	// - 2+ large messages (compressed, flag 0x01 if gzip accepted)
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

			// Decompress and verify
			decompressed, err := decompressGzip(frame.Payload)
			if err != nil {
				t.Errorf("Frame %d: failed to decompress gzip: %v", i, err)
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
		t.Error("This suggests per-message compression is not working!")
	}

	if uncompressedCount == 0 {
		t.Errorf("Expected at least 1 uncompressed frame (flag 0x00), got 0")
	}

	t.Logf("✓ Streaming compression test passed: verified flag 0x01 is set on compressed frames")
}
