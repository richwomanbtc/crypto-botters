use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub enum TradeSide {
    BUY,
    SELL,
}

#[derive(Debug, Clone)]
pub struct TradeEvent {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub price: f64,
    pub size: f64,
    pub side: TradeSide,
    pub trade_id: Option<u64>,
}
