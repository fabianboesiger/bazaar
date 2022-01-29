use crate::{
    apis::{Api, ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Symbol, Wallet,
};

use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use rust_decimal::prelude::*;
use sqlx::{sqlite::SqliteConnectOptions, ConnectOptions, SqlitePool};

/// The Store API is a middleware that stores fetched data in a SQLite database.
/// This is very useful for backtesting, as backtests are usually run many times.
pub struct Store<A>
where
    A: Api,
{
    api: A,
    pool: SqlitePool,
    //conn: Mutex<SqliteConnection>,
}

impl<A> Store<A>
where
    A: Api,
{
    pub async fn new(api: A) -> Self {
        std::fs::create_dir_all("./.store").unwrap();

        let mut options = SqliteConnectOptions::new()
            .filename(format!("./.store/{}.db", A::NAME))
            .create_if_missing(true);

        options.disable_statement_logging();

        //let conn = Mutex::new(SqliteConnection::connect_with(&options).await.unwrap());

        let pool = SqlitePool::connect_with(options).await.unwrap();

        sqlx::query(
            "
                CREATE TABLE IF NOT EXISTS data (
                    market TEXT,
                    timestamp INTEGER,
                    close BLOB,
                    volume BLOB,
                    interval INTEGER,
                    PRIMARY KEY(market, timestamp, interval)
                )
            ",
        )
        .execute(/*&mut *conn.lock().await*/ &pool)
        .await
        .unwrap();

        Store { api, pool }
    }
}

#[async_trait]
impl<A: Api> Api for Store<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = A::LIVE_TRADING_ENABLED;

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        let data: Vec<(String, i64, i64, Option<Vec<u8>>, Option<Vec<u8>>)> = sqlx::query_as(
            "
                    SELECT market, timestamp, interval, close, volume
                    FROM data
                    WHERE market = $1
                    AND timestamp >= $2
                    AND interval = $3
                    ORDER BY timestamp ASC
                    LIMIT 5000
                ",
        )
        .bind(key.market.to_string())
        .bind(key.time.timestamp())
        .bind(key.interval.num_seconds())
        .fetch_all(/*&mut *self.conn.lock().await*/ &self.pool)
        .await
        .unwrap();

        let mut out = Vec::new();
        let mut next_key = key;
        for data in data {
            match data {
                (market, time, interval, Some(close), Some(volume)) => {
                    let curr_key = CandleKey {
                        market: Symbol::new(market),
                        time: Utc.timestamp(time, 0),
                        interval: Duration::seconds(interval),
                    };

                    if curr_key != next_key {
                        break;
                    }
                    out.push((
                        curr_key,
                        Some(Candle {
                            close: blob_to_dec(close),
                            volume: blob_to_dec(volume),
                        }),
                    ));
                }
                (market, time, interval, None, None) => {
                    let curr_key = CandleKey {
                        market: Symbol::new(market),
                        time: Utc.timestamp(time, 0),
                        interval: Duration::seconds(interval),
                    };

                    if curr_key != next_key {
                        break;
                    }

                    out.push((curr_key, None));
                }
                _ => {
                    unreachable!();
                }
            }
            next_key.time = next_key.time + next_key.interval;
        }

        if out.is_empty() {
            log::trace!("Store was empty, fetching using underlying API.");

            let candles = self.api.get_candles(key).await?;
            log::trace!("Got candles!");

            /*
            for (i, candle) in candles.iter().enumerate() {
                let curr_key = CandleKey {
                    time: key.time + key.interval * i as i32,
                    ..key
                };

                sqlx::query("INSERT INTO data (market, timestamp, close, volume, interval) VALUES ($1, $2, $3, $4, $5)")
                    .bind(curr_key.market.to_string())
                    .bind(curr_key.time.timestamp())
                    .bind(candle.as_ref().map(|candle| dec_to_blob(candle.close)))
                    .bind(candle.as_ref().map(|candle| dec_to_blob(candle.volume)))
                    .bind(curr_key.interval.num_seconds())
                    .execute(&mut *self.conn.lock().await).await.unwrap();
            }
            */

            const CHUNK_SIZE: usize = 100;
            for chunk in candles.chunks(CHUNK_SIZE) {
                let mut query_string = String::from(
                    "INSERT OR IGNORE INTO data (market, timestamp, close, volume, interval) VALUES ",
                );
                for (i, _candle) in chunk.iter().enumerate() {
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

                for (curr_key, candle) in chunk.iter() {
                    query = query
                        .bind(curr_key.market.to_string())
                        .bind(curr_key.time.timestamp())
                        .bind(candle.as_ref().map(|candle| dec_to_blob(candle.close)))
                        .bind(candle.as_ref().map(|candle| dec_to_blob(candle.volume)))
                        .bind(curr_key.interval.num_seconds());
                }

                query
                    .execute(/*&mut *self.conn.lock().await*/ &self.pool)
                    .await
                    .unwrap();
            }

            Ok(candles)
        } else {
            Ok(out)
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

    async fn order_fee(&self) -> Decimal {
        self.api.order_fee().await
    }
}

fn blob_to_dec(vec: Vec<u8>) -> Decimal {
    let mut buf = [0; 16];
    buf.clone_from_slice(&vec[..]);
    Decimal::deserialize(buf)
}

fn dec_to_blob(decimal: Decimal) -> Vec<u8> {
    decimal.serialize().to_vec()
}
