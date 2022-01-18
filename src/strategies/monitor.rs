use chrono::{DateTime, Utc};
use futures_util::SinkExt;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::{
    net::TcpStream,
    sync::mpsc::{unbounded_channel, UnboundedSender},
};
use tokio_util::codec::{Framed, LinesCodec};

use crate::{apis::Api, AnyError, ClosedPosition, Exchange, OpenPosition, PreparedPosition};
use std::marker::PhantomData;

use super::{Options, Strategy};

pub struct Monitor<A: Api, S: Strategy<A>> {
    strategy: S,
    phantom: PhantomData<A>,
    sent_closed_positions: usize,
    tx: UnboundedSender<Log>,
}

impl<A: Api, S: Strategy<A>> Monitor<A, S> {
    pub fn new(strategy: S, addr: SocketAddr) -> Self {
        let (tx, mut rx) = unbounded_channel::<Log>();

        tokio::spawn(async move {
            if let Ok(stream) = TcpStream::connect(addr).await {
                let mut stream = Framed::new(stream, LinesCodec::new());

                while let Some(log) = rx.recv().await {
                    stream
                        .send(serde_json::to_string(&log).unwrap())
                        .await
                        .unwrap();
                }
            } else {
                log::error!("Couldn't connect monitor.");

                while let Some(_log) = rx.recv().await {}
            }
        });

        Monitor {
            strategy,
            phantom: PhantomData::default(),
            sent_closed_positions: 0,
            tx,
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
                .send(Log::Init(
                    Self::NAME.to_owned(),
                    A::NAME.to_owned(),
                    A::LIVE_TRADING_ENABLED,
                ))
                .ok();
        }

        result
    }

    fn eval(&mut self, exchange: &mut Exchange<A>) -> Result<(), AnyError> {
        let result = self.strategy.eval(exchange);

        if let Err(err) = &result {
            self.tx.send(Log::Error(format!("{}", err))).ok();
        }

        self.tx
            .send(Log::Equity(Decimal::ZERO, exchange.current_time()))
            .ok();

        for prepared_position in exchange.prepared_positions() {
            self.tx.send(Log::Enter(*prepared_position)).ok();
        }

        for open_position in exchange.open_positions() {
            self.tx.send(Log::Update(*open_position)).ok();
        }

        for prepared_position in exchange.closed_positions().skip(self.sent_closed_positions) {
            self.tx.send(Log::Exit(*prepared_position)).ok();
            self.sent_closed_positions += 1;
        }

        result
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Log {
    // Starts the logging process.
    Init(String, String, bool),
    // The strategy was aborted, including reason.
    Error(String),
    // The strategy enters a position.
    Enter(PreparedPosition),
    // Updates the state of an open position.
    Update(OpenPosition),
    // The strategy exits a position.
    Exit(ClosedPosition),
    // Update the total equity.
    Equity(Decimal, DateTime<Utc>),
}

#[derive(Debug, Clone, Deserialize)]
pub enum Command {
    // Exit all positions and stop execution.
    Stop,
}
