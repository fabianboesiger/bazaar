use crate::{
    apis::Api, AnyError, ClosedPosition, Exchange, LivePosition, OpenedPosition, Position,
    PositionId,
};
use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use std::{collections::HashSet, marker::PhantomData};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use uuid::Uuid;

use super::{Options, Strategy};

pub struct Monitor<A: Api, S: Strategy<A>> {
    strategy: S,
    phantom: PhantomData<A>,
    sent_open_positions: HashSet<PositionId>,
    sent_closed_positions: usize,
    tx: UnboundedSender<Box<dyn Log>>,
    session_id: Uuid,
}

impl<A: Api, S: Strategy<A>> Monitor<A, S> {
    pub fn new(strategy: S) -> Self {
        let (tx, mut rx) = unbounded_channel::<Box<dyn Log>>();
        let session_id = Uuid::new_v4();

        tokio::spawn(async move {
            match PgPoolOptions::new()
                .connect(&env::var("DATABASE_URL").unwrap())
                .await
            {
                Ok(pool) => {
                    while let Some(log) = rx.recv().await {
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
            strategy,
            phantom: PhantomData::default(),
            sent_open_positions: HashSet::new(),
            sent_closed_positions: 0,
            tx,
            session_id,
        }
    }
}

// This strategy is applicable for all APIs that allow futures trading.
impl<A: Api, S: Strategy<A>> Strategy<A> for Monitor<A, S> {
    const NAME: &'static str = S::NAME;

    fn init(&mut self, exchange: &mut Exchange<A>) -> Result<Options, AnyError> {
        let result = self.strategy.init(exchange);

        if let Ok(_options) = &result {
            self.tx
                .send(
                    Session {
                        name: Self::NAME.to_owned(),
                        exchange: A::NAME.to_owned(),
                        live_trading: A::LIVE_TRADING_ENABLED,
                        id: self.session_id,
                    }
                    .boxed(),
                )
                .ok();
        }

        result
    }

    fn eval(&mut self, exchange: &mut Exchange<A>) -> Result<(), AnyError> {
        let result = self.strategy.eval(exchange);

        if let Err(err) = &result {
            self.tx
                .send(
                    Abort {
                        reason: format!("{}", err),
                    }
                    .boxed(),
                )
                .ok();
        }

        if exchange.real_time() || exchange.current_time().minute() == 0 {
            self.tx
                .send(
                    Equity {
                        total: exchange.total(),
                        time: exchange.current_time(),
                    }
                    .boxed(),
                )
                .ok();
        }

        for open_position in exchange.open_positions() {
            if exchange.real_time() || !self.sent_open_positions.contains(&open_position.id()) {
                self.tx.send(open_position.boxed()).ok();
                self.sent_open_positions.insert(open_position.id());
            }
        }

        for closed_position in exchange.closed_positions().skip(self.sent_closed_positions) {
            self.tx.send(closed_position.boxed()).ok();
            self.sent_closed_positions += 1;
            self.sent_open_positions.remove(&closed_position.id());
        }

        result
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

pub struct Abort {
    reason: String,
}

#[async_trait]
impl Log for Abort {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                UPDATE sessions
                SET abort_reason = $2
                WHERE session_id = $1
            ",
        )
        .bind(session_id)
        .bind(&self.reason)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum Command {
    // Exit all positions and stop execution.
    Stop,
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
impl Log for OpenedPosition {
    async fn update(&self, pool: &PgPool, session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                INSERT INTO positions (
                    position_id,
                    session_id,
                    want_size,
                    want_price,
                    market,
                    size,
                    side,
                    open_time,
                    open_price,
                    close_time,
                    close_price,
                    closed
                )
                VALUES (
                    $1,
                    $2,
                    $3,
                    $4,
                    $5,
                    $6,
                    $7,
                    $8,
                    $9,
                    $10,
                    $11,
                    FALSE
                ) 
                ON CONFLICT (position_id) 
                DO UPDATE
                SET close_time = $10,
                close_price = $11
            ",
        )
        .bind(self.id().0)
        .bind(session_id)
        .bind(self.want_size())
        .bind(self.want_price())
        .bind(self.symbol().to_string())
        .bind(self.size())
        .bind(self.side())
        .bind(self.enter_time())
        .bind(self.enter_price())
        .bind(self.exit_time())
        .bind(self.exit_price())
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[async_trait]
impl Log for ClosedPosition {
    async fn update(&self, pool: &PgPool, _session_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "
                UPDATE positions
                SET close_time = $2,
                close_price = $3,
                closed = TRUE
                WHERE position_id = $1
            ",
        )
        .bind(self.id().0)
        .bind(self.exit_time())
        .bind(self.exit_price())
        .execute(pool)
        .await?;

        Ok(())
    }
}
