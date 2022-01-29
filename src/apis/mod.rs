#[cfg(feature = "binance")]
mod binance;
mod forward_fill;
#[cfg(feature = "ftx")]
mod ftx;
mod simulate;
mod store;

#[cfg(feature = "ftx")]
pub use self::ftx::*;
#[cfg(feature = "binance")]
pub use self::ftx::*;
use chrono::{DateTime, Utc};
pub use forward_fill::*;
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

    /// List all markets provided by this API.
    //async fn get_markets(&self) -> Result<Vec<Market>, ApiError>;
    /// Get candles by key.
    /// It must hold that the time of the first candle corresponds to the key, and the rest of the candles correspond to the times in increasing order.
    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError>;
    /// Place order using this API.
    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError>;
    /// Custom formatting for each API.
    fn format_market(&self, market: Symbol) -> String;
    /// Update the current state of the user wallet.
    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError>;
    /// Update the current state of the markets.
    async fn update_markets(&self, market: &mut Markets) -> Result<(), ApiError>;
    async fn order_fee(&self) -> Decimal;
    fn quote_asset(&self) -> Asset;
}

/// Defines an order that can be placed in an exchange.
#[derive(Debug)]
pub struct Order {
    pub market: Symbol,
    pub side: Side,
    pub size: Decimal,
    pub order_type: OrderType,
    pub reduce_only: bool,
    pub time: DateTime<Utc>,
    pub price: Decimal,
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

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};

    use super::*;

    #[tokio::test]
    async fn store_api() {
        let ftx_api = Ftx::new();
        let store_api = Store::new(Ftx::new()).await;

        let key = CandleKey {
            market: Symbol::new("BTC-PERP"),
            time: Utc.ymd(2021, 8, 1).and_hms(0, 0, 0),
            interval: Duration::minutes(1),
        };

        let ftx_candles = ftx_api.get_candles(key).await.unwrap();
        let store_candles = store_api.get_candles(key).await.unwrap();

        assert!(ftx_candles.len() > 100);
        assert!(store_candles.len() > 100);

        assert!(ftx_candles
            .into_iter()
            .zip(store_candles)
            .all(|(a, b)| a == b));
    }

    #[tokio::test]
    async fn simulate_api() {
        let ftx_api = Ftx::new();
        let simulate_api = Simulate::new(Ftx::new(), Wallet::new());

        let key = CandleKey {
            market: Symbol::new("BTC-PERP"),
            time: Utc.ymd(2021, 8, 1).and_hms(0, 0, 0),
            interval: Duration::minutes(1),
        };

        let ftx_candles = ftx_api.get_candles(key).await.unwrap();
        let simulate_candles = simulate_api.get_candles(key).await.unwrap();

        assert!(ftx_candles.len() > 100);
        assert!(simulate_candles.len() > 100);

        assert!(ftx_candles
            .into_iter()
            .zip(simulate_candles)
            .all(|(a, b)| a == b));
    }

    #[tokio::test]
    async fn forward_fill_api() {
        let ftx_api = Ftx::new();
        let forward_fill_api = ForwardFill::new(Ftx::new(), Duration::hours(1));

        let key = CandleKey {
            market: Symbol::new("BTC-PERP"),
            time: Utc.ymd(2021, 8, 1).and_hms(0, 0, 0),
            interval: Duration::minutes(1),
        };

        let ftx_candles = ftx_api.get_candles(key).await.unwrap();
        let forward_fill_candles = forward_fill_api.get_candles(key).await.unwrap();

        assert!(ftx_candles.len() > 100);
        assert!(forward_fill_candles.len() > 100);

        assert!(ftx_candles
            .into_iter()
            .zip(forward_fill_candles)
            .all(|(a, b)| a == b));
    }
}
