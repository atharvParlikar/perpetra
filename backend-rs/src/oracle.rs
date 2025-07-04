use rand::prelude::*;
use rust_decimal::{prelude::FromPrimitive, Decimal};
use rust_decimal_macros::dec;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy)]
pub struct BtcPrice {
    pub timestamp: u64,
    pub price_usd: Decimal,
}

pub struct Oracle {
    price: Decimal,
    rng: StdRng,
}

impl Oracle {
    pub fn new(seed: Option<u64>) -> Self {
        let seed = seed.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        Oracle {
            price: dec!(60_000.0),
            rng: StdRng::seed_from_u64(seed),
        }
    }

    pub fn next_price(&mut self) -> BtcPrice {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // --- Simulate market behavior ---

        // Random daily drift (-1% to +1%)
        let drift = self.rng.random_range(-0.01..0.01);

        // Market noise: small random jitter
        let noise = self
            .rng
            .sample::<f64, _>(rand_distr::Normal::new(0.0, 0.002).unwrap());

        // Occasional large event (spike or crash)
        let event_chance: f64 = self.rng.random();
        let event = if event_chance < 0.005 {
            // 0.5% chance of major move: -20% to +25%
            let magnitude = self.rng.random_range(-0.20..0.25);
            println!("⚠️  Market Event! BTC moved by {:.2}%", magnitude * 100.0);
            magnitude
        } else {
            0.0
        };

        let pct_change = drift + noise + event;

        self.price *= dec!(1.0) + Decimal::from_f64(pct_change).unwrap(); //  HACK: make sure unwrap() never panics
        self.price = self.price.max(dec!(100.0)); // Never go below $100

        BtcPrice {
            timestamp: now,
            price_usd: self.price,
        }
    }
}
