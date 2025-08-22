use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub struct WalletMessage {
    pub wallet_id: String,
    pub amount: Decimal,
}

pub enum WalletEvent {
    Debit(WalletMessage),
    Credit(WalletMessage),
}

pub struct WalletManager {
    balance_map: HashMap<String, Decimal>,
}

impl WalletManager {
    pub fn new() -> Self {
        WalletManager {
            balance_map: HashMap::new(),
        }
    }

    pub fn debit(&mut self, wallet_id: String, amount: Decimal) -> bool {
        if let Some(balance) = self.balance_map.get(&wallet_id) {
            if *balance < amount {
                return false;
            }
        }

        self.balance_map
            .entry(wallet_id)
            .and_modify(|b| *b -= amount)
            .or_insert(dec!(1_000_000));

        true
    }
    pub fn credit(&mut self, wallet_id: String, amount: Decimal) -> bool {
        self.balance_map
            .entry(wallet_id)
            .and_modify(|b| *b += amount);
        // no or_insert here, cuz not possible

        true
    }
}
