package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"math"
	"math/rand"
	"net"
	"net/http"
	"sync"
	"time"
)

type TraderType string

const (
	Scalper           TraderType = "scalper"
	SwingTrader       TraderType = "swing_trader"
	TrendFollower     TraderType = "trend_follower"
	Contrarian        TraderType = "contrarian"
	MarketMaker       TraderType = "market_maker"
	LiquidationHunter TraderType = "liquidation_hunter"
)

type TraderConfig struct {
	Name             string
	Type             TraderType
	MinOrderSize     float64
	MaxOrderSize     float64
	MinLeverage      int
	MaxLeverage      int
	OrderFrequency   float64 // orders per minute
	RiskTolerance    float64 // 0.0 to 1.0
	PriceSensitivity float64 // how much they deviate from current price
}

type OrderRequest struct {
	Type     string  `json:"type_"`
	Amount   float64 `json:"amount"`
	Price    float64 `json:"price"`
	Side     string  `json:"side"`
	Leverage int     `json:"leverage"`
	JWT      string  `json:"jwt"`
}

type TradingBot struct {
	config        TraderConfig
	exchangeURL   string
	client        *http.Client
	lastOrderTime time.Time
	positionSide  *string
	lastPrice     float64
	priceHistory  []float64
	running       bool
	mu            sync.RWMutex
}

func NewTradingBot(config TraderConfig, exchangeURL string) *TradingBot {
	// Create HTTP client with custom transport to prefer IPv4
	transport := &http.Transport{
		DialContext: (&net.Dialer{
			Timeout:   30 * time.Second,
			KeepAlive: 30 * time.Second,
		}).DialContext,
		TLSHandshakeTimeout: 10 * time.Second,
	}

	return &TradingBot{
		config:        config,
		exchangeURL:   exchangeURL,
		client:        &http.Client{Timeout: 10 * time.Second, Transport: transport},
		lastOrderTime: time.Now(),
		lastPrice:     60000.0, // Starting BTC price assumption
		priceHistory:  make([]float64, 0, 100),
		running:       false,
	}
}

func (bot *TradingBot) Start(ctx context.Context) error {
	bot.mu.Lock()
	bot.running = true
	bot.mu.Unlock()

	log.Printf("Starting bot: %s (%s)", bot.config.Name, bot.config.Type)

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
			if !bot.isRunning() {
				return nil
			}

			if err := bot.tradingLoop(); err != nil {
				log.Printf("Error in trading loop for %s: %v", bot.config.Name, err)
			}

			// Sleep for a bit to avoid hammering
			time.Sleep(time.Duration(rand.Intn(400)+100) * time.Millisecond)
		}
	}
}

func (bot *TradingBot) Stop() {
	bot.mu.Lock()
	bot.running = false
	bot.mu.Unlock()
}

func (bot *TradingBot) isRunning() bool {
	bot.mu.RLock()
	defer bot.mu.RUnlock()
	return bot.running
}

func (bot *TradingBot) tradingLoop() error {
	// Check if it's time to place an order
	timeSinceLastOrder := time.Since(bot.lastOrderTime)
	orderInterval := time.Duration(60.0/bot.config.OrderFrequency) * time.Second

	if timeSinceLastOrder >= orderInterval {
		if err := bot.maybePlaceOrder(); err != nil {
			return err
		}
		bot.lastOrderTime = time.Now()
	}

	return nil
}

func (bot *TradingBot) maybePlaceOrder() error {
	// Simulate price movement
	bot.updatePriceSimulation()

	// Each trader type has different logic
	switch bot.config.Type {
	case Scalper:
		return bot.scalperStrategy()
	case SwingTrader:
		return bot.swingTraderStrategy()
	case TrendFollower:
		return bot.trendFollowerStrategy()
	case Contrarian:
		return bot.contrarianStrategy()
	case MarketMaker:
		return bot.marketMakerStrategy()
	case LiquidationHunter:
		return bot.liquidationHunterStrategy()
	default:
		return fmt.Errorf("unknown trader type: %s", bot.config.Type)
	}
}

func (bot *TradingBot) updatePriceSimulation() {
	// Simple random walk around 60k
	change := rand.Float64()*0.04 - 0.02 // ±2% change
	bot.lastPrice *= (1 + change)
	bot.lastPrice = math.Max(50000, math.Min(70000, bot.lastPrice)) // Keep within bounds

	bot.priceHistory = append(bot.priceHistory, bot.lastPrice)
	if len(bot.priceHistory) > 100 {
		bot.priceHistory = bot.priceHistory[1:]
	}
}

func (bot *TradingBot) scalperStrategy() error {
	if rand.Float64() < 0.7 { // 70% chance to trade
		sides := []string{"buy", "sell"}
		side := sides[rand.Intn(len(sides))]
		leverage := rand.Intn(min(5, bot.config.MaxLeverage)-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		// Scalpers place orders close to current price
		priceDeviation := rand.Float64()*0.002 - 0.001 // ±0.1%
		price := bot.lastPrice * (1 + priceDeviation)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) swingTraderStrategy() error {
	if rand.Float64() < 0.3 { // 30% chance to trade
		if len(bot.priceHistory) < 10 {
			return nil
		}

		recentPrices := bot.priceHistory[len(bot.priceHistory)-10:]
		recentHigh := maxFloat64(recentPrices)
		recentLow := minFloat64(recentPrices)
		currentPrice := bot.lastPrice

		var side string
		if currentPrice <= recentLow*1.005 { // Near recent low
			side = "buy"
		} else if currentPrice >= recentHigh*0.995 { // Near recent high
			side = "sell"
		} else {
			return nil // No clear signal
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := currentPrice * (0.998 + rand.Float64()*0.004)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) trendFollowerStrategy() error {
	if rand.Float64() < 0.4 { // 40% chance to trade
		if len(bot.priceHistory) < 20 {
			return nil
		}

		// Simple trend detection
		sellMA := averageFloat64(bot.priceHistory[len(bot.priceHistory)-5:])
		buyMA := averageFloat64(bot.priceHistory[len(bot.priceHistory)-20:])

		var side string
		if sellMA > buyMA*1.01 { // Uptrend
			side = "buy"
		} else if sellMA < buyMA*0.99 { // Downtrend
			side = "sell"
		} else {
			return nil // No clear trend
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := bot.lastPrice * (0.999 + rand.Float64()*0.002)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) contrarianStrategy() error {
	if rand.Float64() < 0.25 { // 25% chance to trade
		if len(bot.priceHistory) < 10 {
			return nil
		}

		// Look for overextension
		oldPrice := bot.priceHistory[len(bot.priceHistory)-10]
		priceChange := (bot.lastPrice - oldPrice) / oldPrice

		var side string
		if priceChange > 0.05 { // 5% up, bet on reversal
			side = "sell"
		} else if priceChange < -0.05 { // 5% down, bet on bounce
			side = "buy"
		} else {
			return nil
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := bot.lastPrice * (0.995 + rand.Float64()*0.01)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) marketMakerStrategy() error {
	if rand.Float64() < 0.8 { // 80% chance to trade
		leverage := rand.Intn(min(3, bot.config.MaxLeverage)-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		// Bid below market, offer above market
		spread := 0.001 + rand.Float64()*0.004 // 0.1% to 0.5% spread

		bidPrice := bot.lastPrice * (1 - spread)
		askPrice := bot.lastPrice * (1 + spread)

		// Sometimes place both sides, sometimes just one
		if rand.Float64() < 0.6 {
			if err := bot.placeOrder("limit", amount, bidPrice, "buy", leverage); err != nil {
				return err
			}
			time.Sleep(100 * time.Millisecond)
			return bot.placeOrder("limit", amount, askPrice, "sell", leverage)
		} else {
			sides := []string{"buy", "sell"}
			side := sides[rand.Intn(len(sides))]
			price := bidPrice
			if side == "sell" {
				price = askPrice
			}
			return bot.placeOrder("limit", amount, price, side, leverage)
		}
	}
	return nil
}

func (bot *TradingBot) liquidationHunterStrategy() error {
	if rand.Float64() < 0.2 { // 20% chance to trade
		leverage := rand.Intn(bot.config.MaxLeverage-max(10, bot.config.MinLeverage)+1) + max(10, bot.config.MinLeverage)
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		// More aggressive price targeting
		sides := []string{"buy", "sell"}
		side := sides[rand.Intn(len(sides))]
		priceDeviation := rand.Float64()*0.02 - 0.01 // ±1%
		price := bot.lastPrice * (1 + priceDeviation)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) placeOrder(orderType string, amount, price float64, side string, leverage int) error {
	orderData := OrderRequest{
		Type:     orderType,
		Amount:   roundFloat(amount, 6),
		Price:    roundFloat(price, 2),
		Side:     side,
		Leverage: leverage,
		JWT:      bot.config.Name,
	}

	jsonData, err := json.Marshal(orderData)
	if err != nil {
		return fmt.Errorf("failed to marshal order data: %v", err)
	}

	// Create request with explicit headers
	req, err := http.NewRequest("POST", bot.exchangeURL+"/order", bytes.NewBuffer(jsonData))
	if err != nil {
		return fmt.Errorf("failed to create request: %v", err)
	}

	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")

	resp, err := bot.client.Do(req)
	if err != nil {
		return fmt.Errorf("failed to send order: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusOK {
		log.Printf("%s: %s %.6f @ %.2f (leverage: %dx) - SUCCESS",
			bot.config.Name, side, amount, price, leverage)
	} else {
		body, _ := io.ReadAll(resp.Body)
		log.Printf("%s: Order failed - %d: %s", bot.config.Name, resp.StatusCode, string(body))
	}

	return nil
}

type BotSwarm struct {
	exchangeURL string
	bots        []*TradingBot
	running     bool
	mu          sync.RWMutex
}

func NewBotSwarm(exchangeURL string) *BotSwarm {
	return &BotSwarm{
		exchangeURL: exchangeURL,
		bots:        make([]*TradingBot, 0),
		running:     false,
	}
}

func (swarm *BotSwarm) createBotConfigs() []TraderConfig {
	var configs []TraderConfig

	// Scalpers (high frequency, low leverage)
	for i := 0; i < 3; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("scalper_%d", i+1),
			Type:             Scalper,
			MinOrderSize:     0.01,
			MaxOrderSize:     0.1,
			MinLeverage:      1,
			MaxLeverage:      5,
			OrderFrequency:   10.0, // 10 orders per minute
			RiskTolerance:    0.2,
			PriceSensitivity: 0.001,
		})
	}

	// Swing traders (medium frequency, medium leverage)
	for i := 0; i < 2; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("swing_%d", i+1),
			Type:             SwingTrader,
			MinOrderSize:     0.05,
			MaxOrderSize:     0.3,
			MinLeverage:      2,
			MaxLeverage:      10,
			OrderFrequency:   2.0, // 2 orders per minute
			RiskTolerance:    0.5,
			PriceSensitivity: 0.005,
		})
	}

	// Trend followers
	for i := 0; i < 2; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("trend_%d", i+1),
			Type:             TrendFollower,
			MinOrderSize:     0.1,
			MaxOrderSize:     0.5,
			MinLeverage:      3,
			MaxLeverage:      15,
			OrderFrequency:   1.5,
			RiskTolerance:    0.6,
			PriceSensitivity: 0.002,
		})
	}

	// Contrarians
	configs = append(configs, TraderConfig{
		Name:             "contrarian_1",
		Type:             Contrarian,
		MinOrderSize:     0.2,
		MaxOrderSize:     0.8,
		MinLeverage:      2,
		MaxLeverage:      12,
		OrderFrequency:   1.0,
		RiskTolerance:    0.7,
		PriceSensitivity: 0.01,
	})

	// Market makers
	for i := 0; i < 2; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("mm_%d", i+1),
			Type:             MarketMaker,
			MinOrderSize:     0.05,
			MaxOrderSize:     0.2,
			MinLeverage:      1,
			MaxLeverage:      3,
			OrderFrequency:   15.0, // Very active
			RiskTolerance:    0.3,
			PriceSensitivity: 0.005,
		})
	}

	// Liquidation hunters (high risk, high leverage)
	configs = append(configs, TraderConfig{
		Name:             "liquidation_hunter",
		Type:             LiquidationHunter,
		MinOrderSize:     0.1,
		MaxOrderSize:     1.0,
		MinLeverage:      20,
		MaxLeverage:      50,
		OrderFrequency:   0.5,
		RiskTolerance:    0.9,
		PriceSensitivity: 0.02,
	})

	return configs
}

func (swarm *BotSwarm) StartSwarm(ctx context.Context) error {
	swarm.mu.Lock()
	defer swarm.mu.Unlock()

	if swarm.running {
		return fmt.Errorf("swarm is already running")
	}

	configs := swarm.createBotConfigs()

	for _, config := range configs {
		bot := NewTradingBot(config, swarm.exchangeURL)
		swarm.bots = append(swarm.bots, bot)
	}

	swarm.running = true
	log.Printf("Starting swarm of %d bots", len(swarm.bots))

	// Start all bots concurrently
	var wg sync.WaitGroup
	for _, bot := range swarm.bots {
		wg.Add(1)
		go func(b *TradingBot) {
			defer wg.Done()
			if err := b.Start(ctx); err != nil && err != context.Canceled {
				log.Printf("Bot %s stopped with error: %v", b.config.Name, err)
			}
		}(bot)
	}

	wg.Wait()
	return nil
}

func (swarm *BotSwarm) StopSwarm() {
	swarm.mu.Lock()
	defer swarm.mu.Unlock()

	if !swarm.running {
		return
	}

	log.Println("Stopping bot swarm")
	swarm.running = false

	for _, bot := range swarm.bots {
		bot.Stop()
	}
}

// Helper functions
func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

func roundFloat(val float64, precision int) float64 {
	ratio := math.Pow(10, float64(precision))
	return math.Round(val*ratio) / ratio
}

func maxFloat64(slice []float64) float64 {
	max := slice[0]
	for _, v := range slice[1:] {
		if v > max {
			max = v
		}
	}
	return max
}

func minFloat64(slice []float64) float64 {
	min := slice[0]
	for _, v := range slice[1:] {
		if v < min {
			min = v
		}
	}
	return min
}

func averageFloat64(slice []float64) float64 {
	sum := 0.0
	for _, v := range slice {
		sum += v
	}
	return sum / float64(len(slice))
}

func main() {
	// Configure your exchange URL here
	exchangeURL := "http://127.0.0.1:3000" // Use 127.0.0.1 instead of localhost to force IPv4

	swarm := NewBotSwarm(exchangeURL)

	// Create a context that can be cancelled
	ctx, cancel := context.WithCancel(context.Background())

	// Handle graceful shutdown
	go func() {
		// You can add signal handling here if needed
		// For now, we'll just run for a while
		time.Sleep(5 * time.Minute) // Run for 5 minutes
		log.Println("Shutting down...")
		cancel()
	}()

	// Start the swarm
	if err := swarm.StartSwarm(ctx); err != nil {
		log.Printf("Error starting swarm: %v", err)
	}

	swarm.StopSwarm()
	log.Println("Bot swarm stopped")
}
