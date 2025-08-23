use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::oneshot;

pub struct WalletOneshotReply {
    pub success: bool,
    pub message: String,
}

pub struct WalletMessage {
    pub wallet_id: String,
    pub amount: Decimal,

    pub oneshot_reply: Option<oneshot::Sender<WalletOneshotReply>>,
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
        println!("[DEBITING] wallet_id: {} amount: {}", wallet_id, amount);
        match self.balance_map.get(&wallet_id) {
            Some(balance) => {
                if *balance < amount {
                    return false;
                }
            }
            None => {
                let initial_balance = dec!(1_000_000);
                self.balance_map.insert(wallet_id.clone(), initial_balance);
                if initial_balance < amount {
                    return false;
                }
            }
        }

        self.balance_map
            .entry(wallet_id)
            .and_modify(|b| *b -= amount)
            .or_insert(dec!(1_000_000));

        true
    }

    pub fn credit(&mut self, wallet_id: String, amount: Decimal) {
        self.balance_map
            .entry(wallet_id)
            .and_modify(|b| *b += amount);
        // no or_insert here, cuz not possible
    }
}
