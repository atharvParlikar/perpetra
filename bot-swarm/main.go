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
	"sync/atomic"
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
	Arbitrageur       TraderType = "arbitrageur"
	MomentumTrader    TraderType = "momentum_trader"
	GridTrader        TraderType = "grid_trader"
	NewsTrader        TraderType = "news_trader"
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
	BuyBias          float64 // 0.0 = sell bias, 0.5 = neutral, 1.0 = buy bias
}

type OrderRequest struct {
	Type     string  `json:"type_"`
	Amount   float64 `json:"amount"`
	Price    float64 `json:"price"`
	Side     string  `json:"side"`
	Leverage int     `json:"leverage"`
	JWT      string  `json:"jwt"`
}

type OrderStats struct {
	TotalBuys  int64
	TotalSells int64
	BuyVolume  float64
	SellVolume float64
	mu         sync.RWMutex
}

func (stats *OrderStats) RecordOrder(side string, amount float64) {
	stats.mu.Lock()
	defer stats.mu.Unlock()

	if side == "buy" {
		atomic.AddInt64(&stats.TotalBuys, 1)
		stats.BuyVolume += amount
	} else {
		atomic.AddInt64(&stats.TotalSells, 1)
		stats.SellVolume += amount
	}
}

func (stats *OrderStats) GetStats() (int64, int64, float64, float64) {
	stats.mu.RLock()
	defer stats.mu.RUnlock()
	return atomic.LoadInt64(&stats.TotalBuys), atomic.LoadInt64(&stats.TotalSells), stats.BuyVolume, stats.SellVolume
}

func (stats *OrderStats) GetImbalance() float64 {
	buys, sells, _, _ := stats.GetStats()
	total := buys + sells
	if total == 0 {
		return 0.0
	}
	return float64(buys-sells) / float64(total)
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
	stats         *OrderStats
	globalStats   *OrderStats
}

func NewTradingBot(config TraderConfig, exchangeURL string, globalStats *OrderStats) *TradingBot {
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
		lastPrice:     60000.0,
		priceHistory:  make([]float64, 0, 100),
		running:       false,
		stats:         &OrderStats{},
		globalStats:   globalStats,
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

			// Randomized sleep to avoid synchronization
			jitter := rand.Intn(200) + 50 // 50-250ms
			time.Sleep(time.Duration(jitter) * time.Millisecond)
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
	bot.updatePriceSimulation()

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
	case Arbitrageur:
		return bot.arbitrageurStrategy()
	case MomentumTrader:
		return bot.momentumTraderStrategy()
	case GridTrader:
		return bot.gridTraderStrategy()
	case NewsTrader:
		return bot.newsTraderStrategy()
	default:
		return fmt.Errorf("unknown trader type: %s", bot.config.Type)
	}
}

func (bot *TradingBot) updatePriceSimulation() {
	change := rand.Float64()*0.04 - 0.02
	bot.lastPrice *= (1 + change)
	bot.lastPrice = math.Max(50000, math.Min(70000, bot.lastPrice))

	bot.priceHistory = append(bot.priceHistory, bot.lastPrice)
	if len(bot.priceHistory) > 100 {
		bot.priceHistory = bot.priceHistory[1:]
	}
}

func (bot *TradingBot) chooseSide() string {
	// Get global imbalance
	imbalance := bot.globalStats.GetImbalance()

	// Adjust buy probability based on imbalance and trader's natural bias
	baseBuyProb := bot.config.BuyBias

	// If too many buys (positive imbalance), reduce buy probability
	// If too many sells (negative imbalance), increase buy probability
	adjustedBuyProb := baseBuyProb - (imbalance * 0.3)

	// Keep within reasonable bounds
	adjustedBuyProb = math.Max(0.1, math.Min(0.9, adjustedBuyProb))

	if rand.Float64() < adjustedBuyProb {
		return "buy"
	}
	return "sell"
}

func (bot *TradingBot) scalperStrategy() error {
	if rand.Float64() < 0.7 {
		side := bot.chooseSide()
		leverage := rand.Intn(min(5, bot.config.MaxLeverage)-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		priceDeviation := rand.Float64()*0.002 - 0.001
		price := bot.lastPrice * (1 + priceDeviation)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) swingTraderStrategy() error {
	if rand.Float64() < 0.3 {
		if len(bot.priceHistory) < 10 {
			return nil
		}

		recentPrices := bot.priceHistory[len(bot.priceHistory)-10:]
		recentHigh := maxFloat64(recentPrices)
		recentLow := minFloat64(recentPrices)
		currentPrice := bot.lastPrice

		var preferredSide string
		if currentPrice <= recentLow*1.005 {
			preferredSide = "buy"
		} else if currentPrice >= recentHigh*0.995 {
			preferredSide = "sell"
		} else {
			preferredSide = bot.chooseSide()
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := currentPrice * (0.998 + rand.Float64()*0.004)

		return bot.placeOrder("limit", amount, price, preferredSide, leverage)
	}
	return nil
}

func (bot *TradingBot) trendFollowerStrategy() error {
	if rand.Float64() < 0.4 {
		if len(bot.priceHistory) < 20 {
			return nil
		}

		shortMA := averageFloat64(bot.priceHistory[len(bot.priceHistory)-5:])
		longMA := averageFloat64(bot.priceHistory[len(bot.priceHistory)-20:])

		var preferredSide string
		if shortMA > longMA*1.01 {
			preferredSide = "buy"
		} else if shortMA < longMA*0.99 {
			preferredSide = "sell"
		} else {
			preferredSide = bot.chooseSide()
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := bot.lastPrice * (0.999 + rand.Float64()*0.002)

		return bot.placeOrder("limit", amount, price, preferredSide, leverage)
	}
	return nil
}

func (bot *TradingBot) contrarianStrategy() error {
	if rand.Float64() < 0.25 {
		if len(bot.priceHistory) < 10 {
			return nil
		}

		oldPrice := bot.priceHistory[len(bot.priceHistory)-10]
		priceChange := (bot.lastPrice - oldPrice) / oldPrice

		var preferredSide string
		if priceChange > 0.05 {
			preferredSide = "sell"
		} else if priceChange < -0.05 {
			preferredSide = "buy"
		} else {
			preferredSide = bot.chooseSide()
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := bot.lastPrice * (0.995 + rand.Float64()*0.01)

		return bot.placeOrder("limit", amount, price, preferredSide, leverage)
	}
	return nil
}

func (bot *TradingBot) marketMakerStrategy() error {
	if rand.Float64() < 0.8 {
		leverage := rand.Intn(min(3, bot.config.MaxLeverage)-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		spread := 0.001 + rand.Float64()*0.004
		bidPrice := bot.lastPrice * (1 - spread)
		askPrice := bot.lastPrice * (1 + spread)

		// Place both sides most of the time to provide liquidity
		if rand.Float64() < 0.6 {
			if err := bot.placeOrder("limit", amount, bidPrice, "buy", leverage); err != nil {
				return err
			}
			time.Sleep(100 * time.Millisecond)
			return bot.placeOrder("limit", amount, askPrice, "sell", leverage)
		} else {
			side := bot.chooseSide()
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
	if rand.Float64() < 0.2 {
		leverage := rand.Intn(bot.config.MaxLeverage-max(10, bot.config.MinLeverage)+1) + max(10, bot.config.MinLeverage)
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		side := bot.chooseSide()
		priceDeviation := rand.Float64()*0.02 - 0.01
		price := bot.lastPrice * (1 + priceDeviation)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) arbitrageurStrategy() error {
	if rand.Float64() < 0.5 {
		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		side := bot.chooseSide()
		priceDeviation := rand.Float64()*0.003 - 0.0015
		price := bot.lastPrice * (1 + priceDeviation)

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) momentumTraderStrategy() error {
	if rand.Float64() < 0.35 {
		if len(bot.priceHistory) < 5 {
			return nil
		}

		recent := bot.priceHistory[len(bot.priceHistory)-5:]
		momentum := (recent[len(recent)-1] - recent[0]) / recent[0]

		var preferredSide string
		if momentum > 0.01 {
			preferredSide = "buy"
		} else if momentum < -0.01 {
			preferredSide = "sell"
		} else {
			preferredSide = bot.chooseSide()
		}

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)
		price := bot.lastPrice * (0.999 + rand.Float64()*0.002)

		return bot.placeOrder("limit", amount, price, preferredSide, leverage)
	}
	return nil
}

func (bot *TradingBot) gridTraderStrategy() error {
	if rand.Float64() < 0.6 {
		leverage := rand.Intn(min(5, bot.config.MaxLeverage)-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		gridSize := 0.002 + rand.Float64()*0.003 // 0.2% to 0.5%
		levels := []float64{-2, -1, 0, 1, 2}
		level := levels[rand.Intn(len(levels))]

		price := bot.lastPrice * (1 + level*gridSize)
		side := bot.chooseSide()

		return bot.placeOrder("limit", amount, price, side, leverage)
	}
	return nil
}

func (bot *TradingBot) newsTraderStrategy() error {
	if rand.Float64() < 0.15 {
		// Simulate news-driven volatility
		volatilityBoost := 1.0 + rand.Float64()*0.5 // Up to 50% more volatility

		leverage := rand.Intn(bot.config.MaxLeverage-bot.config.MinLeverage+1) + bot.config.MinLeverage
		amount := bot.config.MinOrderSize + rand.Float64()*(bot.config.MaxOrderSize-bot.config.MinOrderSize)

		side := bot.chooseSide()
		priceDeviation := (rand.Float64()*0.01 - 0.005) * volatilityBoost
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
		bot.stats.RecordOrder(side, amount)
		bot.globalStats.RecordOrder(side, amount)
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
	globalStats *OrderStats
}

func NewBotSwarm(exchangeURL string) *BotSwarm {
	return &BotSwarm{
		exchangeURL: exchangeURL,
		bots:        make([]*TradingBot, 0),
		running:     false,
		globalStats: &OrderStats{},
	}
}

func (swarm *BotSwarm) createBotConfigs() []TraderConfig {
	var configs []TraderConfig

	// Scalpers (high frequency, low leverage) - 20 bots
	for i := 0; i < 20; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("scalper_%d", i+1),
			Type:             Scalper,
			MinOrderSize:     0.01,
			MaxOrderSize:     0.1,
			MinLeverage:      1,
			MaxLeverage:      5,
			OrderFrequency:   15.0 + rand.Float64()*10.0, // 15-25 orders per minute
			RiskTolerance:    0.2,
			PriceSensitivity: 0.001,
			BuyBias:          0.45 + rand.Float64()*0.1, // 0.45-0.55
		})
	}

	// Swing traders - 15 bots
	for i := 0; i < 15; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("swing_%d", i+1),
			Type:             SwingTrader,
			MinOrderSize:     0.05,
			MaxOrderSize:     0.3,
			MinLeverage:      2,
			MaxLeverage:      10,
			OrderFrequency:   2.0 + rand.Float64()*2.0, // 2-4 orders per minute
			RiskTolerance:    0.5,
			PriceSensitivity: 0.005,
			BuyBias:          0.4 + rand.Float64()*0.2, // 0.4-0.6
		})
	}

	// Trend followers - 12 bots
	for i := 0; i < 12; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("trend_%d", i+1),
			Type:             TrendFollower,
			MinOrderSize:     0.1,
			MaxOrderSize:     0.5,
			MinLeverage:      3,
			MaxLeverage:      15,
			OrderFrequency:   1.5 + rand.Float64()*1.0, // 1.5-2.5 orders per minute
			RiskTolerance:    0.6,
			PriceSensitivity: 0.002,
			BuyBias:          0.45 + rand.Float64()*0.1, // 0.45-0.55
		})
	}

	// Contrarians - 8 bots
	for i := 0; i < 8; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("contrarian_%d", i+1),
			Type:             Contrarian,
			MinOrderSize:     0.2,
			MaxOrderSize:     0.8,
			MinLeverage:      2,
			MaxLeverage:      12,
			OrderFrequency:   1.0 + rand.Float64()*0.5, // 1.0-1.5 orders per minute
			RiskTolerance:    0.7,
			PriceSensitivity: 0.01,
			BuyBias:          0.4 + rand.Float64()*0.2, // 0.4-0.6
		})
	}

	// Market makers - 10 bots
	for i := 0; i < 10; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("mm_%d", i+1),
			Type:             MarketMaker,
			MinOrderSize:     0.05,
			MaxOrderSize:     0.2,
			MinLeverage:      1,
			MaxLeverage:      3,
			OrderFrequency:   20.0 + rand.Float64()*10.0, // 20-30 orders per minute
			RiskTolerance:    0.3,
			PriceSensitivity: 0.005,
			BuyBias:          0.5, // Neutral - they provide both sides
		})
	}

	// Liquidation hunters - 5 bots
	for i := 0; i < 5; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("liquidation_hunter_%d", i+1),
			Type:             LiquidationHunter,
			MinOrderSize:     0.1,
			MaxOrderSize:     1.0,
			MinLeverage:      20,
			MaxLeverage:      50,
			OrderFrequency:   0.5 + rand.Float64()*0.5, // 0.5-1.0 orders per minute
			RiskTolerance:    0.9,
			PriceSensitivity: 0.02,
			BuyBias:          0.45 + rand.Float64()*0.1, // 0.45-0.55
		})
	}

	// Arbitrageurs - 8 bots
	for i := 0; i < 8; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("arbitrageur_%d", i+1),
			Type:             Arbitrageur,
			MinOrderSize:     0.05,
			MaxOrderSize:     0.3,
			MinLeverage:      1,
			MaxLeverage:      5,
			OrderFrequency:   5.0 + rand.Float64()*5.0, // 5-10 orders per minute
			RiskTolerance:    0.4,
			PriceSensitivity: 0.003,
			BuyBias:          0.48 + rand.Float64()*0.04, // 0.48-0.52
		})
	}

	// Momentum traders - 10 bots
	for i := 0; i < 10; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("momentum_%d", i+1),
			Type:             MomentumTrader,
			MinOrderSize:     0.08,
			MaxOrderSize:     0.4,
			MinLeverage:      2,
			MaxLeverage:      12,
			OrderFrequency:   3.0 + rand.Float64()*3.0, // 3-6 orders per minute
			RiskTolerance:    0.6,
			PriceSensitivity: 0.004,
			BuyBias:          0.45 + rand.Float64()*0.1, // 0.45-0.55
		})
	}

	// Grid traders - 12 bots
	for i := 0; i < 12; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("grid_%d", i+1),
			Type:             GridTrader,
			MinOrderSize:     0.03,
			MaxOrderSize:     0.2,
			MinLeverage:      1,
			MaxLeverage:      5,
			OrderFrequency:   8.0 + rand.Float64()*4.0, // 8-12 orders per minute
			RiskTolerance:    0.4,
			PriceSensitivity: 0.002,
			BuyBias:          0.48 + rand.Float64()*0.04, // 0.48-0.52
		})
	}

	// News traders - 6 bots
	for i := 0; i < 6; i++ {
		configs = append(configs, TraderConfig{
			Name:             fmt.Sprintf("news_%d", i+1),
			Type:             NewsTrader,
			MinOrderSize:     0.1,
			MaxOrderSize:     0.8,
			MinLeverage:      3,
			MaxLeverage:      20,
			OrderFrequency:   0.3 + rand.Float64()*0.4, // 0.3-0.7 orders per minute
			RiskTolerance:    0.8,
			PriceSensitivity: 0.015,
			BuyBias:          0.4 + rand.Float64()*0.2, // 0.4-0.6
		})
	}

	// Shuffle the configs to randomize startup order
	rand.Shuffle(len(configs), func(i, j int) { configs[i], configs[j] = configs[j], configs[i] })

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
		bot := NewTradingBot(config, swarm.exchangeURL, swarm.globalStats)
		swarm.bots = append(swarm.bots, bot)
	}

	swarm.running = true
	log.Printf("Starting swarm of %d bots", len(swarm.bots))

	// Start stats reporter
	go swarm.statsReporter(ctx)

	// Start all bots concurrently with staggered startup
	var wg sync.WaitGroup
	for i, bot := range swarm.bots {
		wg.Add(1)
		go func(b *TradingBot, delay int) {
			defer wg.Done()
			// Stagger bot startup to avoid thundering herd
			time.Sleep(time.Duration(delay*50) * time.Millisecond)
			if err := b.Start(ctx); err != nil && err != context.Canceled {
				log.Printf("Bot %s stopped with error: %v", b.config.Name, err)
			}
		}(bot, i)
	}

	wg.Wait()
	return nil
}

func (swarm *BotSwarm) statsReporter(ctx context.Context) {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			buys, sells, buyVol, sellVol := swarm.globalStats.GetStats()
			imbalance := swarm.globalStats.GetImbalance()

			log.Printf("=== TRADING STATS ===")
			log.Printf("Orders: %d buys, %d sells (%.1f%% buy ratio)",
				buys, sells, float64(buys)/float64(buys+sells)*100)
			log.Printf("Volume: %.3f buy, %.3f sell", buyVol, sellVol)
			log.Printf("Imbalance: %.3f (positive = more buys)", imbalance)
			log.Printf("Active bots: %d", len(swarm.bots))
			log.Printf("=====================")
		}
	}
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
	exchangeURL := "http://127.0.0.1:8000"

	swarm := NewBotSwarm(exchangeURL)

	// Create a context that can be cancelled
	ctx, cancel := context.WithCancel(context.Background())

	// Handle graceful shutdown
	go func() {
		time.Sleep(10 * time.Minute) // Run for 10 minutes
		log.Println("Shutting down...")
		cancel()
	}()

	// Start the swarm
	if err := swarm.StartSwarm(ctx); err != nil {
		log.Printf("Error starting swarm: %v", err)
	}

	swarm.StopSwarm()

	// Final stats
	buys, sells, buyVol, sellVol := swarm.globalStats.GetStats()
	log.Printf("=== FINAL STATS ===")
	log.Printf("Total orders: %d buys, %d sells", buys, sells)
	log.Printf("Total volume: %.3f buy, %.3f sell", buyVol, sellVol)
	log.Printf("Buy ratio: %.2f%%", float64(buys)/float64(buys+sells)*100)
	log.Printf("Volume ratio: %.2f%% buy volume", buyVol/(buyVol+sellVol)*100)
	log.Println("Bot swarm stopped")
}
