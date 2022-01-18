use super::Api;
use crate::{
    apis::{ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Symbol, Wallet,
};
use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures_util::lock::Mutex;
use rust_decimal::Decimal;

pub struct ForwardFill<A>
where
    A: Api,
{
    cache: Mutex<HashMap<(Symbol, Duration), (DateTime<Utc>, Candle)>>,
    api: A,
    max_duration: Duration,
}

impl<A> ForwardFill<A>
where
    A: Api,
{
    pub fn new(api: A, max_duration: Duration) -> Self {
        ForwardFill {
            cache: Mutex::new(HashMap::new()),
            api,
            max_duration,
        }
    }
}

#[async_trait]
impl<A: Api> Api for ForwardFill<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = false;

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        let mut candles = self.api.get_candles(key).await?;
        let mut cache = self.cache.lock().await;

        if candles.is_empty() {
            if key.time >= Utc::now() - key.interval * 2 {
                // Do not forward fill candles in the future.
                Ok(Vec::new())
            } else if let Some((time, candle)) = cache.get(&(key.market, key.interval)) {
                if key.time.signed_duration_since(*time) <= self.max_duration {
                    log::warn!("Forward filling candle for time {}.", key.time);
                    Ok(vec![(key, Some(candle.clone()))])
                } else {
                    panic!("Gap too large to forward fill.");
                }
            } else {
                Ok(vec![(key, None)])
            }
        } else {
            for (key, maybe_candle) in candles.iter_mut() {
                if let Some(candle) = maybe_candle {
                    cache.insert((key.market, key.interval), (key.time, candle.clone()));
                } else {
                    if key.time >= Utc::now() - key.interval * 2 {
                        // Do not forward fill candles in the future.
                        break;
                    } else if let Some((time, candle)) = cache.get(&(key.market, key.interval)) {
                        if key.time.signed_duration_since(*time) <= self.max_duration {
                            log::warn!("Forward filling candle for time {}.", key.time);
                            *maybe_candle = Some(candle.clone());
                        } else {
                            panic!("Gap too large forward fill.");
                        }
                    }
                }
            }

            Ok(candles)
        }
    }

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        self.api.place_order(order).await
    }

    fn format_market(&self, market: Symbol) -> String {
        self.api.format_market(market)
    }

    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError> {
        self.api.update_wallet(wallet).await
    }

    async fn update_markets(&self, markets: &mut Markets) -> Result<(), ApiError> {
        self.api.update_markets(markets).await
    }

    fn quote_asset(&self) -> Asset {
        self.api.quote_asset()
    }

    async fn order_fee(&self) -> Decimal {
        self.api.order_fee().await
    }
}
