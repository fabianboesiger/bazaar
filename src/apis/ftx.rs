use super::{Order, OrderInfo};
use crate::{
    apis::{Api, ApiError, OrderType},
    Asset, Candle, CandleKey, MarketInfo, Markets, Side, Symbol, Wallet,
};
use async_trait::async_trait;
use chrono::Utc;
use ftx::{
    options::{Endpoint, Options},
    rest::{GetHistoricalPrices, GetWalletBalances, PlaceOrder, Rest},
};
use rust_decimal::prelude::*;
use std::env;

pub struct Ftx {
    rest: Rest,
    //options: Options,
}

impl Ftx {
    pub fn new() -> Self {
        let options = Options {
            endpoint: env::var("FTX_ENDPOINT")
                .map(|endpoint| match endpoint.to_ascii_lowercase().as_str() {
                    "us" => Endpoint::Us,
                    "com" => Endpoint::Com,
                    _ => panic!("Invalid FTX endpoint specified."),
                })
                .unwrap_or(Endpoint::Com),
            key: env::var("FTX_API_KEY").ok(),
            secret: env::var("FTX_API_SECRET").ok(),
            subaccount: env::var("FTX_SUBACCOUNT").ok(),
        };

        Ftx {
            rest: Rest::new(options),
            //options,
        }
    }
}

#[async_trait]
impl Api for Ftx {
    const NAME: &'static str = "FTX";
    const LIVE_TRADING_ENABLED: bool = true;

    /*
    async fn markets(&self) -> Result<Vec<Market>, ApiError> {
        Ok(self.rest
            .request(GetMarkets {})
            .await
            .map_err(|err| ApiError::Api)?
            .into_iter()
            .filter(|market| market.market_type == MarketType::Future)
            .map(|market| Market::Perp(Asset::new(market.underlying.unwrap())))
            .collect())
    }
    */

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        let req = GetHistoricalPrices {
            market_name: self.format_market(key.market),
            resolution: key.interval.num_seconds() as u32,
            limit: Some(5000),
            start_time: Some(key.time),
            end_time: Some(key.time + key.interval * 5000),
        };

        let candles: Vec<(CandleKey, Candle)> = self
            .rest
            .request(req.clone())
            .await
            .expect(&format!("Request failed for: {:?}", req))
            .into_iter()
            .map(|candle| {
                (
                    CandleKey {
                        time: candle.start_time,
                        ..key
                    },
                    Candle {
                        close: candle.close,
                        volume: candle.volume,
                    },
                )
            })
            .collect();

        let mut out = Vec::new();
        let mut next_key = key;
        /*
        for (key, candle) in candles {
            if next_key != key {
                break;
            }
            out.push(Some(candle));
            next_key.time  = next_key.time + next_key.interval;
        }
        for _ in out.len()..5000 {
            out.push(None);
        }
        */

        'result_loop: for (curr_key, candle) in candles {
            while next_key != curr_key {
                log::trace!("Got NO candle for time {}", next_key.time);
                out.push((next_key, None));
                next_key.time = next_key.time + next_key.interval;
                if next_key.time >= key.time + key.interval * 5000 {
                    break 'result_loop;
                }
            }
            assert_eq!(next_key, curr_key);
            log::trace!("Got candle for time {}", next_key.time);
            out.push((curr_key, Some(candle)));
            next_key.time = next_key.time + next_key.interval;
        }
        for _ in out.len()..5000 {
            // Do not fill candles in the future with none.
            if next_key.time >= Utc::now() - next_key.interval * 2 {
                break;
            }
            out.push((next_key, None));
            next_key.time = next_key.time + next_key.interval;
        }

        Ok(out)
    }
    /*
    async fn price_update(&self, asset: Asset) -> Box<dyn Stream<Item = Candle>> {
        let mut ws = Ws::connect(self.options.clone())
            .await
            .unwrap();

        ws.subscribe(vec![
            Channel::Orders()
        ]);
    }
    */

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        let is_market_order = order.order_type == OrderType::Market;
        self.rest
            .request(PlaceOrder {
                market: self.format_market(order.market),
                side: match order.side {
                    Side::Long => ftx::rest::Side::Buy,
                    Side::Short => ftx::rest::Side::Sell,
                },
                price: match order.order_type {
                    OrderType::Market => None,
                    OrderType::Limit(price) => Some(price),
                },
                r#type: match order.order_type {
                    OrderType::Market => ftx::rest::OrderType::Market,
                    OrderType::Limit(_) => ftx::rest::OrderType::Limit,
                },
                size: order.size,
                reduce_only: order.reduce_only,
                ioc: is_market_order,
                post_only: !is_market_order,
                ..Default::default()
            })
            .await
            .map(|info| OrderInfo {
                price: info.avg_fill_price.unwrap(),
                size: info.filled_size.unwrap_or(Decimal::ZERO),
                time: info.created_at,
            })
            .map_err(|err| match err {
                ftx::rest::Error::Api(_) => ApiError::Api,
                ftx::rest::Error::PlacingLimitOrderRequiresPrice => ApiError::Api,
                ftx::rest::Error::NoSecretConfigured => ApiError::Api,
                ftx::rest::Error::SerdeQs(_) => ApiError::Api,
                ftx::rest::Error::Reqwest(_) => ApiError::Network,
                ftx::rest::Error::Json(_) => ApiError::Api,
            })
    }
    /*
    async fn order_update(&self, asset: Asset) -> Pin<Box<dyn Stream<Item = OrderUpdate>>> {
        let mut ws = Ws::connect(self.options.clone())
            .await
            .unwrap();

        ws.subscribe(vec![
            Channel::Orders
        ]).await.unwrap();

        let asset = *asset;

        ws
            .map(|result| {
                result.unwrap()
            })
            .filter(move |(market, data)| {
                futures_util::future::ready(market.as_ref().unwrap() == asset)
            })
            .map(|(market, data)| {
                OrderUpdate {

                }
            })
            .boxed()
    }
    */
    fn format_market(&self, market: Symbol) -> String {
        match market {
            //Symbol::Spot(base, quote) => format!("{}/{}", base, quote),
            Symbol::Perp(asset) => format!("{}-PERP", asset),
        }
    }

    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError> {
        let balances = self
            .rest
            .request(GetWalletBalances {})
            .await
            .map_err(|_| ApiError::Network)?;

        let free = balances
            .iter()
            .map(|balance| (Asset::new(&balance.coin), balance.free))
            .collect();

        let total = balances
            .iter()
            .map(|balance| (Asset::new(&balance.coin), balance.total))
            .collect();

        *wallet = Wallet { free, total };

        Ok(())
    }

    async fn update_markets(&self, markets: &mut Markets) -> Result<(), ApiError> {
        markets.markets = self
            .rest
            .request(ftx::rest::GetMarkets {})
            .await
            .map_err(|_| ApiError::Network)?
            .into_iter()
            .filter_map(|market| {
                let symbol = Symbol::perp(market.underlying?);
                Some((
                    symbol,
                    MarketInfo {
                        symbol,
                        min_size: market.min_provide_size,
                        size_increment: market.size_increment,
                        price_increment: market.price_increment,
                        daily_quote_volume: market.quote_volume24h,
                    },
                ))
            })
            .collect();

        Ok(())
    }

    fn quote_asset(&self) -> Asset {
        Asset::new("USD")
    }

    async fn order_fee(&self) -> Decimal {
        // 0.0007 = 0.07%
        Decimal::new(7, 4)
    }
}
