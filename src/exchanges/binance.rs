// A module for communicating with the [Binance API](https://binance-docs.github.io/apidocs/spot/en/).

use crate::traits::*;
use anyhow::Result;
use chrono::{serde::ts_milliseconds, DateTime, Utc};
use generic_api_client::{
    http::*,
    types::{
        event::{Event, MessageEvent},
        private::{
            BalanceUpdateEvent, OrderSide, OrderStatus, OrderType, OrderUpdateEvent,
            PositionUpdateEvent,
        },
        public::{TradeEvent, TradeSide},
    },
    websocket::*,
};
use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_this_or_that::as_f64;
use sha2::Sha256;
use std::{
    marker::PhantomData,
    str::FromStr,
    time::{Duration, SystemTime},
    vec,
};

/// The type returned by [Client::request()].
pub type BinanceRequestResult<T> = Result<T, BinanceRequestError>;
pub type BinanceRequestError = RequestError<&'static str, BinanceHandlerError>;

/// Options that can be set when creating handlers
pub enum BinanceOption {
    /// [Default] variant, does nothing
    Default,
    /// API key
    Key(String),
    /// Api secret
    Secret(String),
    /// Base url for HTTP requests
    HttpUrl(BinanceHttpUrl),
    /// Authentication type for HTTP requests
    HttpAuth(BinanceAuth),
    /// [RequestConfig] used when sending requests.
    /// `url_prefix` will be overridden by [HttpUrl](Self::HttpUrl) unless `HttpUrl` is [BinanceHttpUrl::None].
    RequestConfig(RequestConfig),
    /// Base url for WebSocket connections
    WebSocketUrl(BinanceWebSocketUrl),
    /// [WebSocketConfig] used for creating [WebSocketConnection]s
    /// `url_prefix` will be overridden by [WebSocketUrl](Self::WebSocketUrl) unless `WebSocketUrl` is [BinanceWebSocketUrl::None].
    /// By default, `refresh_after` is set to 12 hours and `ignore_duplicate_during_reconnection` is set to `true`.
    WebSocketConfig(WebSocketConfig),
}

/// A `struct` that represents a set of [BinanceOption] s.
#[derive(Clone, Debug)]
pub struct BinanceOptions {
    /// see [BinanceOption::Key]
    pub key: Option<String>,
    /// see [BinanceOption::Secret]
    pub secret: Option<String>,
    /// see [BinanceOption::HttpUrl]
    pub http_url: BinanceHttpUrl,
    /// see [BinanceOption::HttpAuth]
    pub http_auth: BinanceAuth,
    /// see [BinanceOption::RequestConfig]
    pub request_config: RequestConfig,
    /// see [BinanceOption::WebSocketUrl]
    pub websocket_url: BinanceWebSocketUrl,
    /// see [BinanceOption::WebSocketConfig]
    pub websocket_config: WebSocketConfig,
}

/// A `enum` that represents the base url of the Binance REST API.
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[non_exhaustive]
pub enum BinanceHttpUrl {
    /// `https://api.binance.com`
    Spot,
    /// `https://api1.binance.com`
    Spot1,
    /// `https://api2.binance.com`
    Spot2,
    /// `https://api3.binance.com`
    Spot3,
    /// `https://api4.binance.com`
    Spot4,
    /// `https://testnet.binance.vision`
    SpotTest,
    /// `https://data.binance.com`
    SpotData,
    /// `https://fapi.binance.com`
    FuturesUsdM,
    /// `https://dapi.binance.com`
    FuturesCoinM,
    /// `https://testnet.binancefuture.com`
    FuturesTest,
    /// `https://eapi.binance.com`
    EuropeanOptions,
    /// The url will not be modified by [BinanceRequestHandler]
    None,
}

/// A `enum` that represents the base url of the Binance WebSocket API
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
#[non_exhaustive]
pub enum BinanceWebSocketUrl {
    /// `wss://stream.binance.com:9443`
    Spot9443,
    /// `wss://stream.binance.com:443`
    Spot443,
    /// `wss://testnet.binance.vision`
    SpotTest,
    /// `wss://data-stream.binance.com`
    SpotData,
    /// `wss://ws-api.binance.com:443`
    WebSocket443,
    /// `wss://ws-api.binance.com:9443`
    WebSocket9443,
    /// `wss://fstream.binance.com`
    FuturesUsdM,
    /// `wss://fstream-auth.binance.com`
    FuturesUsdMAuth,
    /// `wss://dstream.binance.com`
    FuturesCoinM,
    /// `wss://stream.binancefuture.com`
    FuturesUsdMTest,
    /// `wss://dstream.binancefuture.com`
    FuturesCoinMTest,
    /// `wss://nbstream.binance.com`
    EuropeanOptions,
    /// The url will not be modified by [BinanceRequestHandler]
    None,
}

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum BinanceAuth {
    Sign,
    Key,
    None,
}

#[derive(Debug)]
pub enum BinanceHandlerError {
    ApiError(BinanceError),
    RateLimitError { retry_after: Option<u32> },
    ParseError,
}

#[derive(Deserialize, Debug)]
pub struct BinanceError {
    pub code: i32,
    pub msg: String,
}

/// A `struct` that implements [RequestHandler]
pub struct BinanceRequestHandler<'a, R: DeserializeOwned> {
    options: BinanceOptions,
    _phantom: PhantomData<&'a R>,
}

/// A `struct` that implements [WebSocketHandler]
pub struct BinanceWebSocketHandler<H: FnMut(Vec<Event>) + Send> {
    event_handler: H,
    options: BinanceOptions,
}

// https://binance-docs.github.io/apidocs/spot/en/#general-api-information
impl<'a, B, R> RequestHandler<B> for BinanceRequestHandler<'a, R>
where
    B: Serialize,
    R: DeserializeOwned,
{
    type Successful = R;
    type Unsuccessful = BinanceHandlerError;
    type BuildError = &'static str;

    fn request_config(&self) -> RequestConfig {
        let mut config = self.options.request_config.clone();
        if self.options.http_url != BinanceHttpUrl::None {
            config.url_prefix = self.options.http_url.as_str().to_owned();
        }
        config
    }

    fn build_request(
        &self,
        mut builder: RequestBuilder,
        request_body: &Option<B>,
        _: u8,
    ) -> Result<Request, Self::BuildError> {
        if let Some(body) = request_body {
            let encoded = serde_urlencoded::to_string(body).or(Err(
                "could not serialize body as application/x-www-form-urlencoded",
            ))?;
            builder = builder
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(encoded);
        }

        if self.options.http_auth != BinanceAuth::None {
            // https://binance-docs.github.io/apidocs/spot/en/#signed-trade-user_data-and-margin-endpoint-security
            let key = self.options.key.as_deref().ok_or("API key not set")?;
            builder = builder.header("X-MBX-APIKEY", key);

            if self.options.http_auth == BinanceAuth::Sign {
                let time = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap(); // always after the epoch
                let timestamp = time.as_millis();

                builder = builder.query(&[("timestamp", timestamp)]);

                let secret = self.options.secret.as_deref().ok_or("API secret not set")?;
                let mut hmac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap(); // hmac accepts key of any length

                let mut request = builder.build().or(Err("Failed to build request"))?;
                let query = request.url().query().unwrap(); // we added the timestamp query
                let body = request
                    .body()
                    .and_then(|body| body.as_bytes())
                    .unwrap_or_default();

                hmac.update(&[query.as_bytes(), body].concat());
                let signature = hex::encode(hmac.finalize().into_bytes());

                request
                    .url_mut()
                    .query_pairs_mut()
                    .append_pair("signature", &signature);

                return Ok(request);
            }
        }
        builder.build().or(Err("failed to build request"))
    }

    fn handle_response(
        &self,
        status: StatusCode,
        headers: HeaderMap,
        response_body: Bytes,
    ) -> Result<Self::Successful, Self::Unsuccessful> {
        if status.is_success() {
            serde_json::from_slice(&response_body).map_err(|error| {
                log::debug!("Failed to parse response due to an error: {}", error);
                BinanceHandlerError::ParseError
            })
        } else {
            // https://binance-docs.github.io/apidocs/spot/en/#limits
            if status == 429 || status == 418 {
                let retry_after = if let Some(value) = headers.get("Retry-After") {
                    if let Ok(string) = value.to_str() {
                        if let Ok(retry_after) = u32::from_str(string) {
                            Some(retry_after)
                        } else {
                            log::debug!("Invalid number in Retry-After header");
                            None
                        }
                    } else {
                        log::debug!("Non-ASCII character in Retry-After header");
                        None
                    }
                } else {
                    None
                };
                return Err(BinanceHandlerError::RateLimitError { retry_after });
            }

            let error = match serde_json::from_slice(&response_body) {
                Ok(parsed_error) => BinanceHandlerError::ApiError(parsed_error),
                Err(error) => {
                    log::debug!("Failed to parse error response due to an error: {}", error);
                    BinanceHandlerError::ParseError
                }
            };
            Err(error)
        }
    }
}

impl<H> WebSocketHandler for BinanceWebSocketHandler<H>
where
    H: FnMut(Vec<Event>) + Send + 'static,
{
    fn websocket_config(&self) -> WebSocketConfig {
        let mut config = self.options.websocket_config.clone();
        if self.options.websocket_url != BinanceWebSocketUrl::None {
            config.url_prefix = self.options.websocket_url.as_str().to_owned();
        }
        config
    }

    fn handle_message(&mut self, message: WebSocketMessage) -> Vec<WebSocketMessage> {
        match message {
            WebSocketMessage::Text(message) => {
                match serde_json::from_str::<BinanceWsEvent>(&message) {
                    Ok(event) => match event {
                        BinanceWsEvent::Trade(trade) => {
                            let trade = TradeEvent::from(trade);
                            (self.event_handler)(vec![Event::Trade(trade)]);
                        }
                        BinanceWsEvent::AggTrade(agg_trade) => {
                            let trade = TradeEvent::from(agg_trade);
                            (self.event_handler)(vec![Event::Trade(trade)]);
                        }
                        BinanceWsEvent::AccountUpdate(account_update) => {
                            let (position_updates, balance_updates): (
                                Vec<PositionUpdateEvent>,
                                Vec<BalanceUpdateEvent>,
                            ) = account_update.into();
                            let events: Vec<Vec<Event>> = vec![
                                position_updates
                                    .into_iter()
                                    .map(Event::PositionUpdate)
                                    .collect(),
                                balance_updates
                                    .into_iter()
                                    .map(Event::BalanceUpdate)
                                    .collect(),
                            ];
                            (self.event_handler)(events.concat());
                        }
                        BinanceWsEvent::OrderTradeUpdate(order_trade_update) => {
                            let order_update = OrderUpdateEvent::from(order_trade_update);
                            (self.event_handler)(vec![Event::OrderUpdate(order_update)]);
                        }
                    },
                    Err(_) => {
                        (self.event_handler)(vec![Event::Message(MessageEvent { message })]);
                    }
                }
            }
            WebSocketMessage::Binary(_) => log::debug!("Unexpected binary message received"),
            WebSocketMessage::Ping(_) | WebSocketMessage::Pong(_) => (),
        }
        vec![]
    }
}

impl BinanceHttpUrl {
    /// The URL that this variant represents.
    #[inline(always)]
    fn as_str(&self) -> &'static str {
        match self {
            Self::Spot => "https://api.binance.com",
            Self::Spot1 => "https://api1.binance.com",
            Self::Spot2 => "https://api2.binance.com",
            Self::Spot3 => "https://api3.binance.com",
            Self::Spot4 => "https://api4.binance.com",
            Self::SpotTest => "https://testnet.binance.vision",
            Self::SpotData => "https://data.binance.com",
            Self::FuturesUsdM => "https://fapi.binance.com",
            Self::FuturesCoinM => "https://dapi.binance.com",
            Self::FuturesTest => "https://testnet.binancefuture.com",
            Self::EuropeanOptions => "https://eapi.binance.com",
            Self::None => "",
        }
    }
}

impl BinanceWebSocketUrl {
    /// The URL that this variant represents.
    #[inline(always)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Spot9443 => "wss://stream.binance.com:9443",
            Self::Spot443 => "wss://stream.binance.com:443",
            Self::SpotTest => "wss://testnet.binance.vision",
            Self::SpotData => "wss://data-stream.binance.com",
            Self::WebSocket443 => "wss://ws-api.binance.com:443",
            Self::WebSocket9443 => "wss://ws-api.binance.com:9443",
            Self::FuturesUsdM => "wss://fstream.binance.com",
            Self::FuturesUsdMAuth => "wss://fstream-auth.binance.com",
            Self::FuturesCoinM => "wss://dstream.binance.com",
            Self::FuturesUsdMTest => "wss://stream.binancefuture.com",
            Self::FuturesCoinMTest => "wss://dstream.binancefuture.com",
            Self::EuropeanOptions => "wss://nbstream.binance.com",
            Self::None => "",
        }
    }
}

impl HandlerOptions for BinanceOptions {
    type OptionItem = BinanceOption;

    fn update(&mut self, option: Self::OptionItem) {
        match option {
            BinanceOption::Default => (),
            BinanceOption::Key(v) => self.key = Some(v),
            BinanceOption::Secret(v) => self.secret = Some(v),
            BinanceOption::HttpUrl(v) => self.http_url = v,
            BinanceOption::HttpAuth(v) => self.http_auth = v,
            BinanceOption::RequestConfig(v) => self.request_config = v,
            BinanceOption::WebSocketUrl(v) => self.websocket_url = v,
            BinanceOption::WebSocketConfig(v) => self.websocket_config = v,
        }
    }
}

impl Default for BinanceOptions {
    fn default() -> Self {
        let mut websocket_config = WebSocketConfig::new();
        websocket_config.refresh_after = Duration::from_secs(60 * 60 * 12);
        websocket_config.ignore_duplicate_during_reconnection = true;
        Self {
            key: None,
            secret: None,
            http_url: BinanceHttpUrl::None,
            http_auth: BinanceAuth::None,
            request_config: RequestConfig::default(),
            websocket_url: BinanceWebSocketUrl::None,
            websocket_config,
        }
    }
}

impl<'a, R, B> HttpOption<'a, R, B> for BinanceOption
where
    R: DeserializeOwned + 'a,
    B: Serialize,
{
    type RequestHandler = BinanceRequestHandler<'a, R>;

    #[inline(always)]
    fn request_handler(options: Self::Options) -> Self::RequestHandler {
        BinanceRequestHandler::<'a, R> {
            options,
            _phantom: PhantomData,
        }
    }
}

impl<H: FnMut(Vec<Event>) + Send + 'static> WebSocketOption<H> for BinanceOption {
    type WebSocketHandler = BinanceWebSocketHandler<H>;

    #[inline(always)]
    fn websocket_handler(handler: H, options: Self::Options) -> Self::WebSocketHandler {
        BinanceWebSocketHandler {
            event_handler: handler,
            options,
        }
    }
}

impl HandlerOption for BinanceOption {
    type Options = BinanceOptions;
}

impl Default for BinanceOption {
    fn default() -> Self {
        Self::Default
    }
}
#[derive(Deserialize)]
#[serde(tag = "e")]
enum BinanceWsEvent {
    #[serde(rename = "trade")]
    Trade(BinanceTrade),
    #[serde(rename = "aggTrade")]
    AggTrade(BinanceAggTrade),
    #[serde(rename = "ACCOUNT_UPDATE")]
    AccountUpdate(BinanceAccountUpdate),
    #[serde(rename = "ORDER_TRADE_UPDATE")]
    OrderTradeUpdate(BinanceOrderTradeUpdate),
}

#[derive(Debug, Deserialize)]
struct BinanceTrade {
    // {
    //   "e": "trade",     // Event type
    //   "E": 1672515782136,   // Event time
    //   "s": "BNBBTC",    // Symbol
    //   "t": 12345,       // Trade ID
    //   "p": "0.001",     // Price
    //   "q": "100",       // Quantity
    //   "T": 1672515782136,   // Trade time
    //   "m": true,        // Is the buyer the market maker?
    //   "M": true         // Ignore
    // }
    #[serde(rename = "T", with = "ts_milliseconds")]
    trade_time: DateTime<Utc>,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
    #[serde(rename = "p", deserialize_with = "as_f64")]
    price: f64,
    #[serde(rename = "q", deserialize_with = "as_f64")]
    quantity: f64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "t")]
    trade_id: u64,
}

impl From<BinanceTrade> for TradeEvent {
    fn from(trade: BinanceTrade) -> Self {
        Self {
            timestamp: trade.trade_time,
            symbol: trade.symbol,
            price: trade.price,
            size: trade.quantity,
            trade_id: Some(trade.trade_id),
            side: if trade.is_buyer_maker {
                TradeSide::SELL
            } else {
                TradeSide::BUY
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct BinanceAggTrade {
    // {
    //   "e": "aggTrade",  // Event type
    //   "E": 1672515782136,   // Event time
    //   "s": "BNBBTC",    // Symbol
    //   "a": 12345,       // Aggregate trade ID
    //   "p": "0.001",     // Price
    //   "q": "100",       // Quantity
    //   "f": 100,         // First trade ID
    //   "l": 105,         // Last trade ID
    //   "T": 1672515782136,   // Trade time
    //   "m": true,        // Is the buyer the market maker?
    //   "M": true         // Ignore
    // }
    #[serde(rename = "T", with = "ts_milliseconds")]
    trade_time: DateTime<Utc>,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
    #[serde(rename = "p", deserialize_with = "as_f64")]
    price: f64,
    #[serde(rename = "q", deserialize_with = "as_f64")]
    quantity: f64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "a")]
    trade_id: u64,
}

impl From<BinanceAggTrade> for TradeEvent {
    fn from(trade: BinanceAggTrade) -> Self {
        Self {
            timestamp: trade.trade_time,
            symbol: trade.symbol,
            price: trade.price,
            size: trade.quantity,
            trade_id: Some(trade.trade_id),
            side: if trade.is_buyer_maker {
                TradeSide::SELL
            } else {
                TradeSide::BUY
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct BinanceAccountUpdate {
    //{
    //   "e": "ACCOUNT_UPDATE",                // Event Type
    //   "E": 1564745798939,                   // Event Time
    //   "T": 1564745798938 ,                  // Transaction
    //   "a":                                  // Update Data
    //     {
    //       ...
    //     }
    // }
    #[serde(rename = "T", with = "ts_milliseconds")]
    transaction_timestamp: DateTime<Utc>,
    #[serde(rename = "a")]
    update_data: BinanceAccountUpdateData,
}

#[derive(Debug, Deserialize)]
struct BinanceAccountUpdateData {
    //{
    //  "m":"ORDER",                      // Event reason type
    //  "B":[ ... ],                      // Balances
    //  "P":[ ... ]                        // Positions
    //}
    #[serde(rename = "B")]
    balances: Vec<BinanceBalance>,
    #[serde(rename = "P")]
    positions: Vec<BinancePosition>,
}

#[derive(Debug, Deserialize)]
struct BinanceBalance {
    //{
    //  "a":"USDT",                   // Asset
    //  "wb":"122624.12345678",       // Wallet Balance
    //  "cw":"100.12345678",          // Cross Wallet Balance
    //  "bc":"50.12345678"            // Balance Change except PnL and Commission
    //}
    #[serde(rename = "a")]
    asset: String,
    #[serde(rename = "wb", deserialize_with = "as_f64")]
    wallet_balance: f64,
}

#[derive(Debug, Deserialize)]
struct BinancePosition {
    //{
    //  "s":"BTCUSDT",            // Symbol
    //  "pa":"0",                 // Position Amount
    //  "ep":"0.00000",           // Entry Price
    //  "bep":"0",                // breakeven price
    //  "cr":"200",               // (Pre-fee) Accumulated Realized
    //  "up":"0",                 // Unrealized PnL
    //  "mt":"isolated",          // Margin Type
    //  "iw":"0.00000000",        // Isolated Wallet (if isolated position)
    //  "ps":"BOTH"               // Position Side
    //}
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "pa", deserialize_with = "as_f64")]
    position_amount: f64,
}

impl From<BinanceAccountUpdate> for (Vec<PositionUpdateEvent>, Vec<BalanceUpdateEvent>) {
    fn from(value: BinanceAccountUpdate) -> Self {
        let mut position_updates = Vec::new();
        let mut balance_updates = Vec::new();
        for balance in value.update_data.balances {
            balance_updates.push(BalanceUpdateEvent {
                timestamp: value.transaction_timestamp,
                asset: balance.asset,
                balance: balance.wallet_balance,
            });
        }
        for position in value.update_data.positions {
            position_updates.push(PositionUpdateEvent {
                timestamp: value.transaction_timestamp,
                symbol: position.symbol,
                position: position.position_amount,
            });
        }
        (position_updates, balance_updates)
    }
}

#[derive(Debug, Deserialize)]
struct BinanceOrderTradeUpdate {
    // {
    //   "e":"ORDER_TRADE_UPDATE",     // Event Type
    //   "E":1568879465651,            // Event Time
    //   "T":1568879465650,            // Transaction Time
    //   "o": {...}
    // }
    #[serde(rename = "o")]
    order_trade_update: BinanceOrderTradeUpdateData,
}

#[derive(Debug, Deserialize)]
struct BinanceOrderTradeUpdateData {
    //{
    //     "s":"BTCUSDT",              // Symbol
    //     "c":"TEST",                 // Client Order Id
    //       // special client order id:
    //       // starts with "autoclose-": liquidation order
    //       // "adl_autoclose": ADL auto close order
    //       // "settlement_autoclose-": settlement order for delisting or delivery
    //     "S":"SELL",                 // Side
    //     "o":"TRAILING_STOP_MARKET", // Order Type
    //     "f":"GTC",                  // Time in Force
    //     "q":"0.001",                // Original Quantity
    //     "p":"0",                    // Original Price
    //     "ap":"0",                   // Average Price
    //     "sp":"7103.04",             // Stop Price. Please ignore with TRAILING_STOP_MARKET order
    //     "x":"NEW",                  // Execution Type
    //     "X":"NEW",                  // Order Status
    //     "i":8886774,                // Order Id
    //     "l":"0",                    // Order Last Filled Quantity
    //     "z":"0",                    // Order Filled Accumulated Quantity
    //     "L":"0",                    // Last Filled Price
    //     "N":"USDT",                 // Commission Asset, will not push if no commission
    //     "n":"0",                    // Commission, will not push if no commission
    //     "T":1568879465650,          // Order Trade Time
    //     "t":0,                      // Trade Id
    //     "b":"0",                    // Bids Notional
    //     "a":"9.91",                 // Ask Notional
    //     "m":false,                  // Is this trade the maker side?
    //     "R":false,                  // Is this reduce only
    //     "wt":"CONTRACT_PRICE",      // Stop Price Working Type
    //     "ot":"TRAILING_STOP_MARKET",// Original Order Type
    //     "ps":"LONG",                // Position Side
    //     "cp":false,                 // If Close-All, pushed with conditional order
    //     "AP":"7476.89",             // Activation Price, only puhed with TRAILING_STOP_MARKET order
    //     "cr":"5.0",                 // Callback Rate, only puhed with TRAILING_STOP_MARKET order
    //     "pP": false,                // If price protection is turned on
    //     "si": 0,                    // ignore
    //     "ss": 0,                    // ignore
    //     "rp":"0",                   // Realized Profit of the trade
    //     "V":"EXPIRE_TAKER",         // STP mode
    //     "pm":"OPPONENT",            // Price match mode
    //     "gtd":0                     // TIF GTD order auto cancel time
    //   }
    //}
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "S")]
    side: String,
    #[serde(rename = "o")]
    order_type: String,
    #[serde(rename = "q", deserialize_with = "as_f64")]
    original_quantity: f64,
    #[serde(rename = "p", deserialize_with = "as_f64")]
    original_price: f64,
    #[serde(rename = "X")]
    order_status: String,
    #[serde(rename = "i")]
    order_id: u64,
    #[serde(rename = "T", with = "ts_milliseconds")]
    order_trade_time: DateTime<Utc>,
}

impl From<BinanceOrderTradeUpdate> for OrderUpdateEvent {
    fn from(order: BinanceOrderTradeUpdate) -> Self {
        Self {
            timestamp: order.order_trade_update.order_trade_time,
            symbol: order.order_trade_update.symbol,
            order_id: order.order_trade_update.order_id.to_string(),
            client_order_id: "".to_string(),
            order_type: match order.order_trade_update.order_type.as_str() {
                "LIMIT" => OrderType::Limit,
                "MARKET" => OrderType::Market,
                "STOP" => OrderType::Stop,
                "STOP_MARKET" => OrderType::StopLimit,
                "TAKE_PROFIT" => OrderType::TakeProfit,
                "TAKE_PROFIT_MARKET" => OrderType::TakeProfitLimit,
                _ => OrderType::Unknown,
            },
            side: match order.order_trade_update.side.as_str() {
                "BUY" => OrderSide::Buy,
                "SELL" => OrderSide::Sell,
                _ => OrderSide::Unknown,
            },
            price: order.order_trade_update.original_price,
            size: order.order_trade_update.original_quantity,
            status: match order.order_trade_update.order_status.as_str() {
                "NEW" | "PARTIALLY_FILLED" => OrderStatus::Open,
                "FILLED" => OrderStatus::Filled,
                "EXPIRED" | "CANCELED" | "REJECTED" | "EXPIRED_IN_MATCH" => OrderStatus::Expired,
                _ => OrderStatus::Unknown,
            },
        }
    }
}
