use crate::{types::trade::TradeEvent, websocket::WebSocketMessage};

#[derive(Debug, Clone)]
pub struct ErrorEvent {
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Trade(TradeEvent),
    Error(ErrorEvent),
}
