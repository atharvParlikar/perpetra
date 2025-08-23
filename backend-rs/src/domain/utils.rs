use chrono::{Duration, Timelike, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::time::sleep;

pub fn EMA(p: Decimal, previous_ema: Decimal, alpha: Decimal) -> Decimal {
    alpha * p + (dec!(1) - alpha) * previous_ema
}

pub async fn scheduler() {
    loop {
        let now = Utc::now();
        let current_hour = now.hour();

        // Find next target hour (0, 8, 16)
        let next_hour = match current_hour {
            0..=7 => 8,
            8..=15 => 16,
            _ => 24, // means midnight (next day 0)
        };

        let mut next = now.date_naive().and_hms_opt(next_hour % 24, 0, 0).unwrap();
        if next_hour == 24 {
            // shift to next day at 0:00
            next = next + Duration::days(1);
        }

        let sleep_duration = (next - now.naive_utc()).to_std().unwrap();

        sleep(sleep_duration).await;
    }
}
