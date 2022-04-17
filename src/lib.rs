#![deny(unused_must_use)]
#![deny(unsafe_code)]
#![allow(clippy::new_without_default)]
#![allow(clippy::comparison_chain)]

pub mod apis;
mod asset;
mod candle;
mod exchange;
mod market;
mod order;
pub mod strategies;
mod wallet;

pub use asset::*;
pub use candle::*;
use chrono::{DateTime, Duration, TimeZone, Utc};
pub use exchange::*;
pub use market::*;
pub use order::*;
use rust_decimal_macros::dec;
pub use wallet::*;

use apis::{Api, ForwardFill, Monitor, Simulate, Store};
use rust_decimal::Decimal;
use strategies::Strategy;

pub struct Bazaar {
    /// The start capital for simulated backtesting in USD.
    pub start_capital: Decimal,
    /// The start time for backtesting.
    pub start_time: DateTime<Utc>,
    /// The maximum forward fill duration for backtesting.
    pub forward_fill: Duration,
}

impl Default for Bazaar {
    fn default() -> Self {
        Bazaar {
            start_capital: dec!(1000),
            start_time: Utc.ymd(2021, 1, 1).and_hms(0, 0, 0),
            forward_fill: Duration::days(1),
        }
    }
}

impl Bazaar {
    /// Runs your strategy live.
    #[cfg(not(feature = "backtest"))]
    pub async fn run<A, S>(self, api: A, strategy: S) -> Result<(), AnyError>
    where
        A: Api,
        S: Strategy<Monitor<A>>,
    {
        let api = Monitor::new(api);
        let exchange = Exchange::new(api, self.start_time);
        exchange.run(strategy).await?;

        Ok(())
    }

    /// Runs your strategy in backtest mode.
    /// Exchange data is stored locally to speed up backtesting.
    /// Missing candles are forward filled.
    #[cfg(feature = "backtest")]
    pub async fn run<A, S>(self, api: A, strategy: S) -> Result<(), AnyError>
    where
        A: Api,
        S: Strategy<Monitor<Simulate<ForwardFill<Store<A>>>>>,
    {
        let mut wallet = Wallet::new();
        wallet.deposit(self.start_capital, Asset::new("USD"));

        let api = Monitor::new(Simulate::new(
            ForwardFill::new(Store::new(api).await, self.forward_fill),
            wallet,
        ));
        let exchange = Exchange::new(api, self.start_time);
        exchange.run(strategy).await?;

        Ok(())
    }
}
