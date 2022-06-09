use super::Api;
use crate::{
    apis::{ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, MarketInfo, Markets, Symbol, Wallet,
};

use async_trait::async_trait;
use rust_decimal::prelude::*;

pub trait CandleGen: Fn(CandleKey) -> Candle + Send + Sync {}

pub struct Settings<F>
where
    F: CandleGen,
{
    fee: Decimal,
    candles: F,
    markets: Vec<MarketInfo>,
}

/// The Simulate API is a middleware that does not actually execute orders,
/// and instead simulates the orders.
/// This is useful for backtesting.
pub struct Mock<F>
where
    F: CandleGen,
{
    //orderbooks: HashMap<Symbol, Orderbook>,
    settings: Settings<F>,
}

impl<F> Mock<F>
where
    F: CandleGen,
{
    /// Create a simulation middleware for an api by providing a wallet
    /// with your deposit to simulate, and the fee per orders.
    pub fn new(settings: Settings<F>) -> Self {
        Mock {
            //orderbooks: HashMap::new(),
            settings,
        }
    }
}

#[async_trait]
impl<F> Api for Mock<F>
where
    F: CandleGen,
{
    const NAME: &'static str = "Mock";
    const LIVE_TRADING_ENABLED: bool = false;

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        Ok(vec![(key, Some((self.settings.candles)(key)))])
    }

    async fn update_markets(&self, markets: &mut Markets) -> Result<(), ApiError> {
        *markets = Markets {
            markets: self
                .settings
                .markets
                .iter()
                .map(|market_info| (market_info.symbol, market_info.clone()))
                .collect(),
        };

        Ok(())
    }

    async fn place_order(&self, _order: Order) -> Result<OrderInfo, ApiError> {
        unimplemented!()
    }

    fn format_market(&self, market: Symbol) -> String {
        match market {
            Symbol::Perp(asset) => format!("{}-PERP", asset),
        }
    }

    async fn update_wallet(&self, _wallet: &mut Wallet) -> Result<(), ApiError> {
        unimplemented!()
    }

    fn quote_asset(&self) -> Asset {
        Asset::new("USD")
    }

    async fn order_fee(&self) -> Decimal {
        self.settings.fee
    }
}
