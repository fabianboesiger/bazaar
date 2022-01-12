use super::Api;
use crate::{
    apis::{ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Symbol, Wallet,
};
use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures_util::lock::Mutex;
use rust_decimal::prelude::*;

pub struct Simulate<A>
where
    A: Api,
{
    wallet: Wallet,
    api: A,
    //orderbooks: HashMap<Symbol, Orderbook>,
    order_fee: Decimal,
    cache: Mutex<(
        Option<(DateTime<Utc>, Duration)>,
        HashMap<Symbol, Option<Candle>>,
    )>,
}

impl<A> Simulate<A>
where
    A: Api,
{
    /// Create a simulation middleware for an api by providing a wallet
    /// with your deposit to simulate, and the fee per orders.
    pub fn new(api: A, wallet: Wallet, order_fee: Decimal) -> Self {
        Simulate {
            wallet,
            api,
            //orderbooks: HashMap::new(),
            order_fee,
            cache: Mutex::new((None, HashMap::new())),
        }
    }
}

#[async_trait]
impl<A: Api> Api for Simulate<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = false;

    async fn get_candle(&self, key: CandleKey) -> Result<Option<Candle>, ApiError> {
        let candle = self.api.get_candle(key).await?;

        let mut cache = self.cache.lock().await;
        if let Some((current_time, interval)) = &mut cache.0 {
            if *current_time != key.time {
                assert_eq!(*interval, key.interval);
                *current_time = *current_time + key.interval;
                assert_eq!(*current_time, key.time);
            }
        } else {
            cache.0 = Some((key.time, key.interval));
        }

        cache.1.insert(key.market, candle.clone());

        Ok(candle)
    }

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        let cache = self.cache.lock().await;
        assert_eq!(cache.0.unwrap().0, order.time);
        let candle = cache.1.get(&order.market).unwrap().unwrap();

        Ok(OrderInfo {
            size: order.size * (Decimal::one() - self.order_fee),
            price: candle.close,
            time: order.time,
        })
    }
    /*
    async fn order_update(&self, asset: Asset) -> Pin<Box<dyn Stream<Item = OrderUpdate>>> {
        self.api.order_update(asset).await
    }
    */
    fn format_market(&self, market: Symbol) -> String {
        self.api.format_market(market)
    }

    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError> {
        if wallet.is_fresh() {
            *wallet = self.wallet.clone();
        }

        Ok(())
    }

    async fn update_markets(&self, markets: &mut Markets) -> Result<(), ApiError> {
        /*
        markets.markets
            .iter_mut()
            .for_each(|(_symbol, info)| {
                /*
                let candle = cache.1.get(&symbol).unwrap().unwrap();
                let mut bids = BTreeMap::new();
                let mut asks = BTreeMap::new();
                bids.insert(candle.close, Decimal::new(i64::MAX, 0));
                asks.insert(candle.close, Decimal::new(i64::MAX, 0));
                let orderbook = Orderbook { bids, asks };
                */

                *info = MarketInfo {
                    min_size: Decimal::zero(),
                    size_increment: Decimal::zero(),
                    price_increment: Decimal::zero(),
                    daily_volume: Decimal::new(i64::MAX, 0),
                };
            });
        */
        if markets.is_fresh() {
            self.api.update_markets(markets).await?;
        }

        Ok(())
    }

    fn quote_asset(&self) -> Asset {
        self.api.quote_asset()
    }
}
