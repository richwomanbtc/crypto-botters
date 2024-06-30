use chrono::{DateTime, Utc};
#[derive(Debug, Clone)]
pub struct PositionUpdateEvent {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub position: f64,
}
#[derive(Debug, Clone)]
pub struct BalanceUpdateEvent {
    pub timestamp: DateTime<Utc>,
    pub asset: String,
    pub balance: f64,
}

#[derive(Debug, Clone)]
pub enum OrderType {
    Limit,
    Market,
    Stop,
    StopLimit,
    TakeProfit,
    TakeProfitLimit,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum OrderSide {
    Buy,
    Sell,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum OrderStatus {
    Open,
    Filled,
    Expired,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct OrderUpdateEvent {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub order_id: String,
    pub client_order_id: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub price: f64,
    pub size: f64,
    pub status: OrderStatus,
}
