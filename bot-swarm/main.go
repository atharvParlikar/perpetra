package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"math/rand"
	"net"
	"net/http"
	"sync"
	"sync/atomic"
	"time"
)

// ----- tweakables -----
const (
	exchangeURL   = "http://127.0.0.1:8000" // your server
	users         = 59                      // concurrent goroutines
	testDuration  = 2 * time.Minute         // run time
	orderEndpoint = "/order"
)

// keep it simple: one order shape
type OrderRequest struct {
	Type     string  `json:"type_"`
	Amount   float64 `json:"amount"`
	Price    float64 `json:"price"`
	Side     string  `json:"side"`
	Leverage int     `json:"leverage"`
	JWT      string  `json:"jwt"`
}

func main() {
	// fast-ish HTTP client with keep-alives
	transport := &http.Transport{
		Proxy:               http.ProxyFromEnvironment,
		DialContext:         (&net.Dialer{Timeout: 5 * time.Second, KeepAlive: 30 * time.Second}).DialContext,
		MaxIdleConns:        5000,
		MaxIdleConnsPerHost: 2000,
		IdleConnTimeout:     90 * time.Second,
		ForceAttemptHTTP2:   false, // keep it simple: HTTP/1.1
	}
	client := &http.Client{
		Timeout:   10 * time.Second,
		Transport: transport,
	}

	ctx, cancel := context.WithTimeout(context.Background(), testDuration)
	defer cancel()

	var (
		totalSent     int64
		totalOK       int64
		totalFail     int64
		totalBuy      int64
		totalSell     int64
		totalLatencyN int64 // number of latencies recorded
		totalLatency  int64 // sum of latencies in microseconds (atomic-friendly)
	)

	var wg sync.WaitGroup
	wg.Add(users)

	startWall := time.Now()

	for u := 0; u < users; u++ {
		userID := u
		go func() {
			defer wg.Done()
			r := rand.New(rand.NewSource(time.Now().UnixNano() + int64(userID)))

			for {
				select {
				case <-ctx.Done():
					return
				default:
					side := "buy"
					if r.Intn(2) == 0 {
						side = "sell"
					}

					// keep values very simple & bounded
					order := OrderRequest{
						Type:     "limit",
						Amount:   round6(0.01 + r.Float64()*0.99),  // 0.01..1.00
						Price:    round2(59000 + r.Float64()*2000), // ~59000..61000
						Side:     side,
						Leverage: 1 + r.Intn(10),                 // 1..10
						JWT:      fmt.Sprintf("user_%d", userID), // placeholder "auth"
					}

					body, _ := json.Marshal(order)
					req, _ := http.NewRequest("POST", exchangeURL+orderEndpoint, bytes.NewReader(body))
					req.Header.Set("Content-Type", "application/json")
					req.Header.Set("Accept", "application/json")

					start := time.Now()
					resp, err := client.Do(req)
					elapsed := time.Since(start)

					atomic.AddInt64(&totalSent, 1)
					atomic.AddInt64(&totalLatencyN, 1)
					atomic.AddInt64(&totalLatency, elapsed.Microseconds())

					if side == "buy" {
						atomic.AddInt64(&totalBuy, 1)
					} else {
						atomic.AddInt64(&totalSell, 1)
					}

					if err != nil {
						atomic.AddInt64(&totalFail, 1)
						continue
					}

					io.Copy(io.Discard, resp.Body)
					resp.Body.Close()

					if resp.StatusCode == http.StatusOK {
						atomic.AddInt64(&totalOK, 1)
					} else {
						atomic.AddInt64(&totalFail, 1)
					}

					time.Sleep(time.Duration(5+r.Intn(15)) * time.Millisecond)
				}
			}
		}()
	}

	wg.Wait()
	wall := time.Since(startWall)

	// final stats
	sent := atomic.LoadInt64(&totalSent)
	ok := atomic.LoadInt64(&totalOK)
	fail := atomic.LoadInt64(&totalFail)
	buys := atomic.LoadInt64(&totalBuy)
	sells := atomic.LoadInt64(&totalSell)
	latN := atomic.LoadInt64(&totalLatencyN)
	latSumUS := atomic.LoadInt64(&totalLatency)
	avgLatMS := 0.0
	if latN > 0 {
		avgLatMS = float64(latSumUS) / float64(latN) / 1000.0
	}
	rps := float64(sent) / wall.Seconds()

	fmt.Println("=== LOAD TEST SUMMARY ===")
	fmt.Printf("Duration:          %s\n", wall.Truncate(time.Millisecond))
	fmt.Printf("Users (goroutines): %d\n", users)
	fmt.Printf("Requests sent:     %d\n", sent)
	fmt.Printf("  - 200 OK:        %d\n", ok)
	fmt.Printf("  - Fail/Non-200:  %d\n", fail)
	fmt.Printf("Avg latency:       %.2f ms\n", avgLatMS)
	fmt.Printf("Throughput:        %.1f req/s\n", rps)
	fmt.Printf("Side split:        buys=%d  sells=%d (buy ratio %.1f%%)\n", buys, sells,
		percent(float64(buys), float64(buys+sells)))
	fmt.Println("=========================")
}

func round2(f float64) float64 { return float64(int64(f*100+0.5)) / 100 }
func round6(f float64) float64 { return float64(int64(f*1e6+0.5)) / 1e6 }
func percent(a, total float64) float64 {
	if total <= 0 {
		return 0
	}
	return a / total * 100.0
}
