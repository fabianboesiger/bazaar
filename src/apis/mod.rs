#[cfg(feature = "binance")]
mod binance;
mod forward_fill;
#[cfg(feature = "ftx")]
mod ftx;
mod monitor;
mod simulate;
mod store;

#[cfg(feature = "binance")]
pub use self::binance::*;
#[cfg(feature = "ftx")]
pub use self::ftx::*;
pub use forward_fill::*;
pub use monitor::*;
pub use simulate::*;
pub use store::*;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::*;
use thiserror::Error;

use crate::{Asset, Candle, CandleKey, Markets, Order, OrderInfo, Symbol, Wallet};
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
    fn hello(&self, _strategy_name: &'static str) {}
    fn status(&self, _time: DateTime<Utc>, _total: Decimal) {}
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Could not connect to the API.")]
    Network,
    #[error("Internal API error.")]
    Api,
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::*;

    #[tokio::test]
    async fn store_api() {
        let ftx_api = Ftx::from_env();
        let store_api = Store::new(Ftx::from_env()).await;

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
        let ftx_api = Ftx::from_env();
        let simulate_api = Simulate::new(Ftx::from_env(), Wallet::new());

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
        let ftx_api = Ftx::from_env();
        let forward_fill_api = ForwardFill::new(Ftx::from_env(), Duration::hours(1));

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
