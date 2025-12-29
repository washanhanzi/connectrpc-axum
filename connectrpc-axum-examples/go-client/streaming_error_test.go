package main

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

// TestStreamingError verifies that streaming handlers returning errors
// BEFORE the stream starts produce proper Connect streaming responses:
// - HTTP 200 with Content-Type: application/connect+json
// - Error in EndStream frame (flags=0x02)
func TestStreamingError(t *testing.T) {
	s := startServer(t, "streaming-error-repro", "")
	defer s.stop()

	tests := []struct {
		name        string
		requestName string
		wantErrCode string // empty means expect success
	}{
		{"unauthorized error", "unauthorized", "permission_denied"},
		{"invalid argument error", "invalid", "invalid_argument"},
		{"not found error", "notfound", "not_found"},
		{"normal stream succeeds", "Alice", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := sendStreamRequest(t, tt.requestName)

			if tt.wantErrCode != "" {
				// Expecting error
				if !result.hasEndStream {
					t.Fatal("Missing EndStream frame")
				}
				if result.errCode != tt.wantErrCode {
					t.Fatalf("Expected error %q, got %q", tt.wantErrCode, result.errCode)
				}
				t.Logf("Got expected error: %s", result.errCode)
			} else {
				// Expecting success
				if !result.hasMessages {
					t.Fatal("No messages received")
				}
				if !result.hasEndStream {
					t.Fatal("Missing EndStream frame")
				}
				t.Logf("Received %d message frames", result.messageCount)
			}
		})
	}
}

type streamResult struct {
	statusCode   int
	contentType  string
	hasEndStream bool
	hasMessages  bool
	messageCount int
	errCode      string
	err          error
}

func sendStreamRequest(t *testing.T, name string) streamResult {
	url := serverURL + "/hello.HelloWorldService/SayHelloStream"

	// Build Connect streaming envelope: [flags:1][length:4][payload]
	jsonPayload := []byte(fmt.Sprintf(`{"name":"%s"}`, name))
	envelope := make([]byte, 5+len(jsonPayload))
	envelope[0] = 0x00 // flags (no compression, not end stream)
	binary.BigEndian.PutUint32(envelope[1:5], uint32(len(jsonPayload)))
	copy(envelope[5:], jsonPayload)

	req, err := http.NewRequest("POST", url, strings.NewReader(string(envelope)))
	if err != nil {
		return streamResult{err: err}
	}
	// Streaming requests must use application/connect+json per Connect protocol spec
	req.Header.Set("Content-Type", "application/connect+json")

	client := &http.Client{Timeout: 10 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return streamResult{err: err}
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return streamResult{err: err}
	}

	result := streamResult{
		statusCode:  resp.StatusCode,
		contentType: resp.Header.Get("Content-Type"),
	}

	// Streaming must return 200
	if resp.StatusCode != http.StatusOK {
		result.err = fmt.Errorf("expected HTTP 200, got %d", resp.StatusCode)
		return result
	}

	// Must be application/connect+json
	if !strings.HasPrefix(result.contentType, "application/connect+json") {
		result.err = fmt.Errorf("expected application/connect+json, got %s", result.contentType)
		return result
	}

	// Parse Connect streaming frames
	parseStreamFrames(t, body, &result)
	return result
}

func parseStreamFrames(t *testing.T, body []byte, result *streamResult) {
	offset := 0

	for offset < len(body) {
		if len(body)-offset < 5 {
			break
		}

		flags := body[offset]
		length := binary.BigEndian.Uint32(body[offset+1 : offset+5])
		offset += 5

		isEndStream := flags&0x02 != 0

		if int(length) > len(body)-offset {
			break
		}

		payload := body[offset : offset+int(length)]
		offset += int(length)

		// Parse JSON payload
		var jsonData map[string]interface{}
		if err := json.Unmarshal(payload, &jsonData); err != nil {
			continue
		}

		// Check if it's an error frame
		if errData, ok := jsonData["error"]; ok {
			if errMap, ok := errData.(map[string]interface{}); ok {
				if code, ok := errMap["code"].(string); ok {
					result.errCode = code
				}
			}
		} else if _, ok := jsonData["message"]; ok {
			result.hasMessages = true
			result.messageCount++
		}

		if isEndStream {
			result.hasEndStream = true
			break
		}
	}
}
