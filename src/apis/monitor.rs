use super::Api;
use crate::{
    apis::{ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Symbol, Wallet,
};
use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use uuid::Uuid;

pub struct Monitor<A>
where
    A: Api,
{
    api: A,
    tx: UnboundedSender<Box<dyn Log>>,
    session_id: Uuid,
}

impl<A> Monitor<A>
where
    A: Api,
{
    pub fn new(api: A) -> Self {
        let (tx, mut rx) = unbounded_channel::<Box<dyn Log>>();
        let session_id = Uuid::new_v4();

        tokio::spawn(async move {
            match PgPoolOptions::new()
                .connect(&env::var("DATABASE_URL").unwrap())
                .await
            {
                Ok(pool) => {
                    while let Some(log) = rx.recv().await {
                        log::trace!("monitor update");
                        if let Err(err) = log.update(&pool, session_id).await {
                            log::error!("A database error occurred: {}", err);
                        }
                    }
                }
                Err(_) => {
                    log::error!("Failed to connect to monitor database.");
                    while let Some(_log) = rx.recv().await {
                        // Discard log.
                    }
                }
            }
        });

        Monitor {
            api,
            tx,
            session_id,
        }
    }
}

#[async_trait]
impl<A: Api> Api for Monitor<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = A::LIVE_TRADING_ENABLED;

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        self.api.get_candles(key).await
    }

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        log::trace!("place order monitor");

        self.tx.send(order.clone().boxed()).ok();

        let order_info = self.api.place_order(order).await?;

        self.tx.send(order_info.clone().boxed()).ok();

        Ok(order_info)
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

    fn hello(&self, strategy_name: &'static str) {
        self.tx
            .send(
                Session {
                    name: strategy_name.to_owned(),
                    exchange: A::NAME.to_owned(),
                    live_trading: A::LIVE_TRADING_ENABLED,
                    id: self.session_id,
                }
                .boxed(),
            )
            .ok();
    }

    fn status(&self, time: DateTime<Utc>, total: Decimal) {
        if time.minute() == 0 {
            self.tx.send(Equity { total, time }.boxed()).ok();
        }
    }
}

#[async_trait]
pub trait Log: Send + Sync {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error>;
    fn boxed(self) -> Box<dyn Log>
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: Uuid,
    name: String,
    exchange: String,
    live_trading: bool,
}

#[async_trait]
impl Log for Session {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
        assert_eq!(self.id, session_id);

        sqlx::query(
            "
                INSERT INTO sessions (session_id, name, exchange, live_trading)
                VALUES ($1, $2, $3, $4)
            ",
        )
        .bind(self.id)
        .bind(&self.name)
        .bind(&self.exchange)
        .bind(self.live_trading)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equity {
    total: Decimal,
    time: DateTime<Utc>,
}

#[async_trait]
impl Log for Equity {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                INSERT INTO equities (session_id, total, time)
                VALUES ($1, $2, $3)
            ",
        )
        .bind(session_id)
        .bind(self.total)
        .bind(self.time)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl Log for Order {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                INSERT INTO orders (
                    order_id,
                    session_id,
                    market,
                    side,
                    ordered_size,
                    ordered_price,
                    ordered_time,
                    executed_size,
                    executed_price,
                    executed_time
                )
                VALUES (
                    $1,
                    $2,
                    $3,
                    $4,
                    $5,
                    $6,
                    $7,
                    NULL,
                    NULL,
                    NULL
                )
            ",
        )
        .bind(self.order_id)
        .bind(session_id)
        .bind(self.market.to_string())
        .bind(self.side)
        .bind(self.size)
        .bind(self.current_price)
        .bind(self.time)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl Log for OrderInfo {
    async fn update(&self, pool: &PgPool, _session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                UPDATE orders 
                SET (
                    executed_size,
                    executed_price,
                    executed_time
                ) = (
                    $2,
                    $3,
                    $4
                ) 
                WHERE order_id = $1
            ",
        )
        .bind(self.order_id)
        .bind(self.size)
        .bind(self.price)
        .bind(self.time)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {}
