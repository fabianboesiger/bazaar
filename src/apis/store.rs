use std::{pin::Pin, ops::DerefMut};

use crate::{
    apis::{Api, ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Symbol, Wallet,
};

use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use futures_util::{lock::Mutex, Stream, StreamExt};
use rust_decimal::prelude::*;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool, SqliteConnection, Error as SqlxError, Connection};
use cht::map::HashMap;
use deadpool::managed::{Manager, RecycleResult, Pool, Object};

struct DbPool {
    options: SqliteConnectOptions,
}

impl DbPool {
    fn new(options: SqliteConnectOptions) -> DbPool {
        DbPool {
            options,
        }
    }
}

#[async_trait]
impl Manager for DbPool {
    type Type = SqliteConnection;
    type Error = SqlxError;

    async fn create(&self) -> Result<SqliteConnection, SqlxError> {
        SqliteConnection::connect_with(&self.options).await
        
    }
    async fn recycle(&self, obj: &mut SqliteConnection) -> RecycleResult<SqlxError> {
        Ok(obj.ping().await?)
    }
}

pub struct Store<A>
where
    A: Api,
{
    api: A,
    pool: Pool<DbPool>,
    cache: HashMap<CandleKey, Option<Candle>>,
}

impl<A> Store<A>
where
    A: Api,
{
    pub async fn new(api: A) -> Self {
        std::fs::create_dir_all("./.store").unwrap();

        let options = SqliteConnectOptions::new()
            .filename(format!("./.store/{}.db", A::NAME))
            .create_if_missing(true);

        let pool: Pool<DbPool, Object<DbPool>> = Pool::builder(DbPool::new(options)).build().unwrap();
        

        sqlx::query(
            "
                CREATE TABLE IF NOT EXISTS data (
                    market TEXT,
                    timestamp INTEGER,
                    close BLOB,
                    volume BLOB,
                    interval INTEGER
                )
            ",
        )
        .execute(pool.get().await.unwrap().deref_mut())
        .await
        .unwrap();

        Store {
            api,
            pool,
            cache: HashMap::new(),
        }
    }
}

#[async_trait]
impl<A: Api> Api for Store<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = A::LIVE_TRADING_ENABLED;

    async fn get_candle(&self, key: CandleKey) -> Result<Option<Candle>, ApiError> {
        if let Some(candle) = self.cache.remove(&key) {
            Ok(candle)
        } else {

            let data: Vec<(String, i64, i64, Option<Vec<u8>>, Option<Vec<u8>>)> = sqlx::query_as(
                "
                    SELECT market, timestamp, interval, close, volume
                    FROM data
                    WHERE market = $1
                    AND timestamp >= $2
                    AND timestamp < $3
                    AND interval = $4
                    ORDER BY timestamp ASC
                ",
            )
            .bind(key.market.to_string())
            .bind(key.time.timestamp())
            .bind((key.time + key.interval * 5000).timestamp())
            .bind(key.interval.num_seconds())
            .fetch_all(self.pool.get().await.unwrap().deref_mut())
            .await
            .unwrap();

            if data.is_empty() {
                log::trace!("Store was empty, fetching using underlying API.");

                let mut query_string = String::from(
                    "INSERT INTO data (market, timestamp, close, volume, interval) VALUES ",
                );
                for i in 0..5000 {
                    query_string += &format!(
                        "(${},${},${},${},${}),",
                        i * 5 + 1,
                        i * 5 + 2,
                        i * 5 + 3,
                        i * 5 + 4,
                        i * 5 + 5,
                    );
                }
                query_string.pop();
                let mut query = sqlx::query(&query_string);

                for i in 0..5000 {
                    log::trace!("Fetching candle {}", i);
                    let fetch_time = key.time + key.interval * i;
                    let fetch_key = CandleKey {
                        time: fetch_time,
                        ..key
                    };
                    let candle = self.api.get_candle(fetch_key).await?;
                    self.cache.insert(fetch_key, candle);

                    query = query
                        .bind(fetch_key.market.to_string())
                        .bind(fetch_key.time.timestamp())
                        .bind(candle.as_ref().map(|candle| dec_to_blob(candle.close)))
                        .bind(candle.as_ref().map(|candle| dec_to_blob(candle.volume)))
                        .bind(fetch_key.interval.num_seconds());
                }
                log::trace!("Getting connection");
                let mut obj = self.pool.get().await.unwrap();
                let connection = obj.deref_mut();
                log::trace!("Executing insert");
                query.execute(connection).await.unwrap();
                log::trace!("Done executing insert");

                Ok(self.cache.remove(&key).unwrap())
            } else {
                log::debug!("Store was non empty, locking.");
                log::debug!("Store was non empty, locked.");

                for data in data {
                    match data {
                        (market, time, interval, Some(close), Some(volume)) => {
                            self.cache.insert(
                                CandleKey {
                                    market: Symbol::new(market),
                                    time: Utc.timestamp(time, 0),
                                    interval: Duration::seconds(interval),
                                },
                                Some(Candle {
                                    close: blob_to_dec(close),
                                    volume: blob_to_dec(volume),
                                }),
                            );
                        }
                        (market, time, interval, None, None) => {
                            self.cache.insert(
                                CandleKey {
                                    market: Symbol::new(market),
                                    time: Utc.timestamp(time, 0),
                                    interval: Duration::seconds(interval),
                                },
                                None,
                            );
                        }
                        _ => {
                            unreachable!();
                        }
                    }
                }

                Ok(self.cache.remove(&key).unwrap())
            }
        }
    }

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        self.api.place_order(order).await
    }
    /*
    async fn order_update(&self, asset: Asset) -> Pin<Box<dyn Stream<Item = OrderUpdate>>> {
        todo!()
    }
    */
    fn format_market(&self, symbol: Symbol) -> String {
        self.api.format_market(symbol)
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
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};

    use super::Store;
    use crate::{
        apis::{ftx::Ftx, Api},
        Asset, CandleKey, Symbol,
    };

    #[tokio::test]
    async fn cache() {
        simple_logger::SimpleLogger::new()
            .with_level(log::LevelFilter::Debug)
            .with_utc_timestamps()
            .init()
            .unwrap();

        let cache = Store::new(Ftx::new()).await;
        let mut time = Utc.ymd(2021, 6, 1).and_hms(0, 0, 0);
        for i in 0..10000 {
            let candle = cache
                .get_candle(CandleKey {
                    market: Symbol::Perp(Asset::new("BTC")),
                    time,
                    interval: Duration::seconds(15),
                })
                .await;

            if candle.unwrap().is_none() {
                panic!("No candle received for time {}.", time);
            }
            time = time + Duration::seconds(15);
        }
    }
}

fn blob_to_dec(vec: Vec<u8>) -> Decimal {
    let mut buf = [0; 16];
    for i in 0..buf.len() {
        buf[i] = vec[i];
    }
    Decimal::deserialize(buf)
}

fn dec_to_blob(decimal: Decimal) -> Vec<u8> {
    decimal.serialize().to_vec()
}
