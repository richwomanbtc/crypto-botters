use crate::{types::trade::TradeEvent, websocket::WebSocketMessage};

#[derive(Debug, Clone)]
pub struct MessageEvent {
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Trade(TradeEvent),
    Message(MessageEvent),
}
