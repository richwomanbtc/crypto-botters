use crate::{types::public::TradeEvent, websocket::WebSocketMessage};

use super::private::{BalanceUpdateEvent, OrderUpdateEvent, PositionUpdateEvent};

#[derive(Debug, Clone)]
pub struct MessageEvent {
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Trade(TradeEvent),
    PositionUpdate(PositionUpdateEvent),
    BalanceUpdate(BalanceUpdateEvent),
    OrderUpdate(OrderUpdateEvent),
    Message(MessageEvent),
}
