#[cfg(feature = "ftx")]
mod ftx;
mod simulate;
mod store;

#[cfg(feature = "ftx")]
pub use self::ftx::*;
use chrono::{DateTime, Utc};
pub use simulate::*;
pub use store::*;

use rust_decimal::prelude::*;
use thiserror::Error;

use crate::{Asset, Candle, CandleKey, Markets, Side, Symbol, Wallet};
use async_trait::async_trait;

#[async_trait]
pub trait Api: Send + Sync {
    const NAME: &'static str;
    const LIVE_TRADING_ENABLED: bool;

    fn quote_asset(&self) -> Asset;
    /// List all markets provided by this API.
    //async fn get_markets(&self) -> Result<Vec<Market>, ApiError>;
    /// Get candle by key.
    async fn get_candle(&self, key: CandleKey) -> Result<Option<Candle>, ApiError>;
    /// Place order using this API.
    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError>;
    /// Custom formatting for each API.
    fn format_market(&self, market: Symbol) -> String;
    /// Update the current state of the user wallet.
    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError>;
    /// Update the current state of the markets.
    async fn update_markets(&self, market: &mut Markets) -> Result<(), ApiError>;
}

#[derive(Debug)]
pub struct Order {
    pub market: Symbol,
    pub side: Side,
    pub size: Decimal,
    pub order_type: OrderType,
    pub reduce_only: bool,
    pub time: DateTime<Utc>,
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Could not connect to the API.")]
    Network,
    #[error("Internal API error.")]
    Api,
}

#[derive(PartialEq, Eq, Debug)]
pub enum OrderType {
    Limit(Decimal),
    Market,
}

pub struct OrderInfo {
    pub size: Decimal,
    pub price: Decimal,
    pub time: DateTime<Utc>,
}
