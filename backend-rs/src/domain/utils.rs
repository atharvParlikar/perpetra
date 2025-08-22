use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub fn EMA(p: Decimal, previous_ema: Decimal, alpha: Decimal) -> Decimal {
    alpha * p + (dec!(1) - alpha) * previous_ema
}
