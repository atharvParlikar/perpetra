# TO-DO

- [x] Margin logic correction
- [x] Simulate price Oracle
- [x] Liquidation detection
- [ ] book-thread dual queue system (liquidation/user-trades)


## Possible improvements

PositionTracker.update_risk()

```rust
pub fn update_risk(&mut self, mark_price: Decimal) {
    // Retain will remove items from self.positions if the closure returns false.
    // The removed items can be processed immediately.
    self.positions.retain_mut(|_user_id, position| {
        position.unrealized_pnl = position.size * (mark_price - position.entry_price);

        let should_liquidate = (position.margin + position.unrealized_pnl) 
            <= position.margin * LIQUIDATION_THRESHOLD;

        if should_liquidate {
            // This position needs to be liquidated.
            // Create the liquidation order here directly.
            let (resp_tx, _) = oneshot::channel();
            let order = Order {
                id: Uuid::new_v4().to_string(),
                user_id: position.user_id.clone(), // Clone is still needed here
                amount: position.size.abs(),
                price: dec!(0),
                order_type: MARKET,
                side: if position.size.is_sign_positive() { ASK } else { BID },
                responder: Some(resp_tx),
            };

            if let Err(e) = self.book_liquidation_tx.send(Message::Order(order)) {
                eprintln!("Failed to send liquidation order for {}: {}", position.user_id, e);
            }

            // Return `false` to remove the position from the map.
            return false;
        }

        // Return `true` to keep the position in the map.
        true
    });
}
```
