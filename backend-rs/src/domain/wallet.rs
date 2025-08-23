use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::oneshot;

use crate::domain::wallet;

pub struct WalletOneshotReply {
    pub success: bool,
    pub message: String,
}

pub struct WalletDebitMessage {
    pub wallet_id: String,
    pub amount: Decimal,

    pub oneshot_reply: Option<oneshot::Sender<WalletOneshotReply>>,
}

pub struct WalletCreditMessage {
    pub wallet_id: String,
    pub amount: Decimal,
}

pub enum WalletEvent {
    Debit(WalletDebitMessage),
    Credit(WalletCreditMessage),
}

pub struct WalletManager {
    balance_map: HashMap<String, Decimal>,
}

impl WalletManager {
    pub fn new() -> Self {
        let mut balance_map = HashMap::new();
        balance_map.insert("exchange".to_string(), dec!(10_000_000));
        WalletManager {
            balance_map: balance_map,
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

    pub fn transfer(&mut self, payment_sender_id: String, payment_reciever_id: String) {}

    pub fn get_balance(self, wallet_id: String) -> Option<String> {
        if let Some(b) = self.balance_map.get(&wallet_id) {
            Some(b.to_string())
        } else {
            None
        }
    }
}
