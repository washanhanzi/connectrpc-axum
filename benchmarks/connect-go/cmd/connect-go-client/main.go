package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"net/http"
	"os"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"connectrpc.com/connect"

	benchv1 "github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/bench/v1"
	"github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/bench/v1/benchv1connect"
	fortunev1 "github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/fortune/v1"
	"github.com/washanhanzi/connectrpc-axum/benchmarks/connect-go/gen/fortune/v1/fortunev1connect"
)

const (
	expectedFortuneCount = 13
	maxLatencySamples    = 500_000
	maxHTTPClients       = 8
)

type config struct {
	baseURL     string
	suite       string
	protocol    string
	payloadSize string
	compression string
	concurrency int
	warmup      time.Duration
	measurement time.Duration
}

type metrics struct {
	RPS   float64 `json:"rps"`
	P50US uint64  `json:"p50_us"`
	P99US uint64  `json:"p99_us"`
}

type requestRunner interface {
	Call() error
}

type protocolRunner struct {
	client  benchv1connect.BenchServiceClient
	request *benchv1.BenchRequest
}

func (r protocolRunner) Call() error {
	response, err := r.client.Unary(context.Background(), connect.NewRequest(r.request))
	if err != nil {
		return fmt.Errorf("protocol request failed: %w", err)
	}
	if err := validateProtocolResponse(r.request, response.Msg); err != nil {
		return err
	}
	return nil
}

type appRunner struct {
	client  fortunev1connect.FortuneServiceClient
	request *fortunev1.GetFortunesRequest
}

func (r appRunner) Call() error {
	response, err := r.client.GetFortunes(context.Background(), connect.NewRequest(r.request))
	if err != nil {
		return fmt.Errorf("app request failed: %w", err)
	}
	if len(response.Msg.Fortunes) != expectedFortuneCount {
		return fmt.Errorf("expected %d fortunes, got %d", expectedFortuneCount, len(response.Msg.Fortunes))
	}
	return nil
}

func main() {
	cfg := parseFlags()
	result, err := runBenchmark(cfg)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func parseFlags() config {
	var cfg config
	flag.StringVar(&cfg.baseURL, "base-url", "", "target server base URL, for example http://127.0.0.1:8080")
	flag.StringVar(&cfg.suite, "suite", "", "benchmark suite: protocol or app")
	flag.StringVar(&cfg.protocol, "protocol", "", "benchmark protocol: grpc, connect-json, or connect-protobuf")
	flag.StringVar(&cfg.payloadSize, "payload-size", "", "request payload size: small, medium, or large")
	flag.StringVar(&cfg.compression, "compression", "", "request/response compression: identity or gzip")
	flag.IntVar(&cfg.concurrency, "concurrency", 0, "number of concurrent workers")
	flag.DurationVar(&cfg.warmup, "warmup", 0, "warmup duration")
	flag.DurationVar(&cfg.measurement, "measurement", 0, "measurement duration")
	flag.Parse()

	if cfg.baseURL == "" {
		exitUsage("missing --base-url")
	}
	if cfg.suite != "protocol" && cfg.suite != "app" {
		exitUsage("unsupported --suite, use protocol or app")
	}
	switch cfg.protocol {
	case "grpc", "connect-json", "connect-protobuf":
	default:
		exitUsage("unsupported --protocol, use grpc, connect-json, or connect-protobuf")
	}
	switch cfg.payloadSize {
	case "small", "medium", "large":
	default:
		exitUsage("unsupported --payload-size, use small, medium, or large")
	}
	switch cfg.compression {
	case "identity", "gzip":
	default:
		exitUsage("unsupported --compression, use identity or gzip")
	}
	if cfg.concurrency <= 0 {
		exitUsage("--concurrency must be greater than zero")
	}
	if cfg.warmup <= 0 {
		exitUsage("--warmup must be greater than zero")
	}
	if cfg.measurement <= 0 {
		exitUsage("--measurement must be greater than zero")
	}

	return cfg
}

func exitUsage(message string) {
	fmt.Fprintf(os.Stderr, "%s\n\n", message)
	flag.Usage()
	os.Exit(2)
}

func runBenchmark(cfg config) (metrics, error) {
	runners, cleanup, err := newRunners(cfg)
	if err != nil {
		return metrics{}, err
	}
	defer cleanup()

	var (
		running   atomic.Bool
		completed atomic.Uint64

		latenciesMu sync.Mutex
		latencies   = make([]uint64, 0, sampleCapacity(cfg.measurement))

		firstErrMu sync.Mutex
		firstErr   error
	)

	running.Store(true)

	var wg sync.WaitGroup
	wg.Add(cfg.concurrency)
	for workerIdx := range cfg.concurrency {
		runner := runners[workerIdx%len(runners)]
		go func(runner requestRunner) {
			defer wg.Done()

			for running.Load() {
				startedAt := time.Now()
				if err := runner.Call(); err != nil {
					firstErrMu.Lock()
					if firstErr == nil {
						firstErr = err
					}
					firstErrMu.Unlock()
					running.Store(false)
					return
				}

				n := completed.Add(1)
				if n%10 == 0 {
					latenciesMu.Lock()
					if len(latencies) < maxLatencySamples {
						latencies = append(latencies, uint64(time.Since(startedAt).Microseconds()))
					}
					latenciesMu.Unlock()
				}
			}
		}(runner)
	}

	time.Sleep(cfg.warmup)
	completed.Store(0)
	latenciesMu.Lock()
	latencies = latencies[:0]
	latenciesMu.Unlock()
	measurementStartedAt := time.Now()

	time.Sleep(cfg.measurement)
	running.Store(false)
	elapsed := time.Since(measurementStartedAt)

	wg.Wait()

	firstErrMu.Lock()
	err = firstErr
	firstErrMu.Unlock()
	if err != nil {
		return metrics{}, err
	}

	latenciesMu.Lock()
	sort.Slice(latencies, func(i, j int) bool {
		return latencies[i] < latencies[j]
	})
	p50US := percentile(latencies, 50)
	p99US := percentile(latencies, 99)
	latenciesMu.Unlock()

	total := completed.Load()
	return metrics{
		RPS:   float64(total) / elapsed.Seconds(),
		P50US: p50US,
		P99US: p99US,
	}, nil
}

func newRunners(cfg config) ([]requestRunner, func(), error) {
	switch cfg.suite {
	case "protocol":
		request := newProtocolRequest(cfg.payloadSize)
		clients, cleanup := newProtocolClients(cfg)
		runners := make([]requestRunner, 0, len(clients))
		for _, client := range clients {
			runners = append(runners, protocolRunner{
				client:  client,
				request: request,
			})
		}
		return runners, cleanup, nil
	case "app":
		request := newAppRequest(cfg.payloadSize)
		clients, cleanup := newAppClients(cfg)
		runners := make([]requestRunner, 0, len(clients))
		for _, client := range clients {
			runners = append(runners, appRunner{
				client:  client,
				request: request,
			})
		}
		return runners, cleanup, nil
	default:
		return nil, nil, fmt.Errorf("unsupported suite %q", cfg.suite)
	}
}

func newProtocolClients(cfg config) ([]benchv1connect.BenchServiceClient, func()) {
	httpClients, cleanup := newHTTPClients(cfg.concurrency)
	opts := clientOptions(cfg)

	clients := make([]benchv1connect.BenchServiceClient, 0, len(httpClients))
	for _, httpClient := range httpClients {
		clients = append(clients, benchv1connect.NewBenchServiceClient(httpClient, cfg.baseURL, opts...))
	}
	return clients, cleanup
}

func newAppClients(cfg config) ([]fortunev1connect.FortuneServiceClient, func()) {
	httpClients, cleanup := newHTTPClients(cfg.concurrency)
	opts := clientOptions(cfg)

	clients := make([]fortunev1connect.FortuneServiceClient, 0, len(httpClients))
	for _, httpClient := range httpClients {
		clients = append(clients, fortunev1connect.NewFortuneServiceClient(httpClient, cfg.baseURL, opts...))
	}
	return clients, cleanup
}

func newHTTPClients(concurrency int) ([]connect.HTTPClient, func()) {
	clientCount := min(concurrency, maxHTTPClients)
	clients := make([]connect.HTTPClient, 0, clientCount)
	transports := make([]*http.Transport, 0, clientCount)

	for range clientCount {
		protocols := new(http.Protocols)
		protocols.SetUnencryptedHTTP2(true)

		transport := &http.Transport{
			DisableCompression:  true,
			MaxConnsPerHost:     1,
			MaxIdleConns:        1,
			MaxIdleConnsPerHost: 1,
			IdleConnTimeout:     90 * time.Second,
			Protocols:           protocols,
		}
		clients = append(clients, &http.Client{Transport: transport})
		transports = append(transports, transport)
	}

	cleanup := func() {
		for _, transport := range transports {
			transport.CloseIdleConnections()
		}
	}

	return clients, cleanup
}

func clientOptions(cfg config) []connect.ClientOption {
	opts := make([]connect.ClientOption, 0, 4)
	switch cfg.protocol {
	case "grpc":
		opts = append(opts, connect.WithGRPC())
	case "connect-json":
		opts = append(opts, connect.WithProtoJSON())
	case "connect-protobuf":
	}

	switch cfg.compression {
	case "identity":
		opts = append(opts, connect.WithAcceptCompression("gzip", nil, nil))
	case "gzip":
		opts = append(opts, connect.WithSendGzip(), connect.WithCompressMinBytes(1))
	}

	return opts
}

func newProtocolRequest(size string) *benchv1.BenchRequest {
	spec := protocolPayloadSpec(size)
	data := strings.Repeat("ABCD", spec.dataRepeats)
	tags := make([]string, 0, spec.tagCount)
	scores := make([]int32, 0, spec.scoreCount)
	attributes := make(map[string]string, spec.attributeCount)
	headers := make(map[string]string, spec.headerCount)

	for i := range spec.tagCount {
		tags = append(tags, fmt.Sprintf("tag-%03d-%s", i, strings.Repeat("bench", spec.textRepeats)))
	}
	for i := range spec.scoreCount {
		scores = append(scores, int32((i%97)+1))
	}
	for i := range spec.attributeCount {
		attributes[fmt.Sprintf("attr-%03d", i)] = strings.Repeat("value", spec.textRepeats)
	}
	for i := range spec.headerCount {
		headers[fmt.Sprintf("x-bench-%03d", i)] = strings.Repeat("hdr", spec.textRepeats)
	}

	return &benchv1.BenchRequest{
		Payload: &benchv1.Payload{
			Id:             42,
			TimestampNanos: 1_737_000_000_123_456_789,
			Latitude:       37.7749,
			Longitude:      -122.4194,
			Active:         true,
			TraceId:        0xDEADBEEFCAFEBABE,
			Name:           strings.Repeat("protocol-bench-name-", spec.textRepeats),
			Description:    strings.Repeat("protocol-benchmark-description-", spec.textRepeats*2),
			Region:         strings.Repeat("us-west-", spec.textRepeats),
			Data:           []byte(data),
			Status:         benchv1.Status_STATUS_ACTIVE,
			Metadata: &benchv1.Metadata{
				RequestId: strings.Repeat("request-id-", spec.textRepeats),
				UserAgent: strings.Repeat("connect-go-bench/", spec.textRepeats),
				CreatedAt: 1_737_000_000,
				Headers:   headers,
			},
			Scores:     scores,
			Tags:       tags,
			Attributes: attributes,
		},
	}
}

func newAppRequest(size string) *fortunev1.GetFortunesRequest {
	spec := appPayloadSpec(size)
	fortuneIDs := make([]int32, 0, spec.idCount)
	searchTerms := make([]string, 0, spec.searchTermCount)
	audienceSegments := make([]string, 0, spec.segmentCount)
	metadata := make(map[string]string, spec.metadataCount)

	for i := range spec.idCount {
		fortuneIDs = append(fortuneIDs, int32(i+1))
	}
	for i := range spec.searchTermCount {
		searchTerms = append(searchTerms, fmt.Sprintf("term-%03d-%s", i, strings.Repeat("fortune", spec.textRepeats)))
	}
	for i := range spec.segmentCount {
		audienceSegments = append(audienceSegments, fmt.Sprintf("segment-%03d-%s", i, strings.Repeat("audience", spec.textRepeats)))
	}
	for i := range spec.metadataCount {
		metadata[fmt.Sprintf("meta-%03d", i)] = strings.Repeat("context", spec.textRepeats)
	}

	return &fortunev1.GetFortunesRequest{
		FortuneIds:       fortuneIDs,
		SearchTerms:      searchTerms,
		AudienceSegments: audienceSegments,
		Metadata:         metadata,
	}
}

func validateProtocolResponse(request *benchv1.BenchRequest, response *benchv1.BenchResponse) error {
	if response.GetPayload() == nil {
		return fmt.Errorf("protocol response payload was nil")
	}
	if request.GetPayload() == nil {
		return fmt.Errorf("protocol request payload was nil")
	}
	if response.GetPayload().GetId() != request.GetPayload().GetId() {
		return fmt.Errorf("protocol response id mismatch: got %d want %d", response.GetPayload().GetId(), request.GetPayload().GetId())
	}
	if len(response.GetPayload().GetData()) != len(request.GetPayload().GetData()) {
		return fmt.Errorf("protocol response data length mismatch: got %d want %d", len(response.GetPayload().GetData()), len(request.GetPayload().GetData()))
	}
	if len(response.GetPayload().GetScores()) != len(request.GetPayload().GetScores()) {
		return fmt.Errorf("protocol response scores length mismatch: got %d want %d", len(response.GetPayload().GetScores()), len(request.GetPayload().GetScores()))
	}
	if len(response.GetPayload().GetTags()) != len(request.GetPayload().GetTags()) {
		return fmt.Errorf("protocol response tags length mismatch: got %d want %d", len(response.GetPayload().GetTags()), len(request.GetPayload().GetTags()))
	}
	if len(response.GetPayload().GetAttributes()) != len(request.GetPayload().GetAttributes()) {
		return fmt.Errorf("protocol response attributes length mismatch: got %d want %d", len(response.GetPayload().GetAttributes()), len(request.GetPayload().GetAttributes()))
	}
	return nil
}

type protocolSpec struct {
	dataRepeats    int
	scoreCount     int
	tagCount       int
	attributeCount int
	headerCount    int
	textRepeats    int
}

func protocolPayloadSpec(size string) protocolSpec {
	switch size {
	case "small":
		return protocolSpec{
			dataRepeats:    16,
			scoreCount:     8,
			tagCount:       4,
			attributeCount: 4,
			headerCount:    2,
			textRepeats:    1,
		}
	case "medium":
		return protocolSpec{
			dataRepeats:    256,
			scoreCount:     64,
			tagCount:       24,
			attributeCount: 16,
			headerCount:    8,
			textRepeats:    6,
		}
	case "large":
		return protocolSpec{
			dataRepeats:    4096,
			scoreCount:     256,
			tagCount:       96,
			attributeCount: 48,
			headerCount:    24,
			textRepeats:    24,
		}
	default:
		panic("unsupported payload size")
	}
}

type appSpec struct {
	idCount         int
	searchTermCount int
	segmentCount    int
	metadataCount   int
	textRepeats     int
}

func appPayloadSpec(size string) appSpec {
	switch size {
	case "small":
		return appSpec{
			idCount:         4,
			searchTermCount: 4,
			segmentCount:    2,
			metadataCount:   4,
			textRepeats:     1,
		}
	case "medium":
		return appSpec{
			idCount:         32,
			searchTermCount: 24,
			segmentCount:    8,
			metadataCount:   16,
			textRepeats:     4,
		}
	case "large":
		return appSpec{
			idCount:         128,
			searchTermCount: 96,
			segmentCount:    32,
			metadataCount:   48,
			textRepeats:     12,
		}
	default:
		panic("unsupported payload size")
	}
}

func sampleCapacity(measurement time.Duration) int {
	perSecondEstimate := int(measurement / time.Second)
	if perSecondEstimate < 1 {
		perSecondEstimate = 1
	}
	capacity := perSecondEstimate * 50_000
	if capacity > maxLatencySamples {
		return maxLatencySamples
	}
	return capacity
}

func percentile(samples []uint64, percentile int) uint64 {
	if len(samples) == 0 {
		return 0
	}
	return samples[len(samples)*percentile/100]
}
