use std::{collections::{HashMap, HashSet}, cell::RefCell};

use super::Wallet;
use crate::{
    apis::{Api, ApiError, Order, OrderType},
    strategies::{OnError, Options, Strategy},
    Candle, CandleKey, Markets, Symbol, MarketInfo,
};
use chrono::{DateTime, Duration, Utc};
use futures_util::{future::join_all, try_join};
use rust_decimal::prelude::*;
use uuid::Uuid;

pub type AnyError = Box<dyn std::error::Error>;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PrepareError {
    #[error("Inufficient free assets available.")]
    InsufficientAssets,
    #[error("Market closed.")]
    MarketClosed,
}

/// This struct keeps track of the state of the exchange, your positions, your wallet etc.
pub struct Exchange<A: Api> {
    wallet: Wallet,
    prepared_positions: Vec<PreparedPosition>,
    open_positions: Vec<OpenPosition>,
    closed_positions: Vec<ClosedPosition>,
    to_close: RefCell<HashSet<PositionId>>,
    candles: HashMap<Symbol, Option<Candle>>,
    markets: Markets,
    current_time: DateTime<Utc>,
    real_time: bool,
    api: A,
}

impl<A: Api> Exchange<A> {
    /// Create an exchange layer using the specified API.
    pub fn new(api: A, start_time: DateTime<Utc>) -> Self {
        Exchange {
            current_time: start_time,
            wallet: Wallet::new(),
            prepared_positions: Vec::new(),
            open_positions: Vec::new(),
            closed_positions: Vec::new(),
            to_close: RefCell::new(HashSet::new()),
            candles: HashMap::new(),
            markets: Markets::new(),
            api,
            real_time: false,
        }
    }

    pub fn current_time(&self) -> DateTime<Utc> {
        self.current_time
    }

    pub fn real_time(&self) -> bool {
        self.real_time
    }

    /// List all available markets.
    pub fn markets(&self) -> impl Iterator<Item = &MarketInfo> {
        self
            .markets
            .markets()
            .map(|(_, info)| info)
    }

    pub fn market(&self, symbol: Symbol) -> &MarketInfo {
        self
            .markets
            .market(symbol)
            .unwrap()
    }

    /// Fetch the current candle of a market.
    pub fn candle(&self, market: Symbol) -> Option<&Candle> {
        self.candles.get(&market)?.as_ref()
    }

    /// Begin watching a market.
    pub fn watch(&mut self, market: Symbol) {
        self.candles.insert(market, None);
    }

    /// Stop watching a market.
    pub fn unwatch(&mut self, market: Symbol) {
        self.candles.remove(&market);
    }

    pub fn prepare(&mut self, position: Position) -> Result<PreparedPosition, PrepareError> {
        let rounded_size = self.round_size(position.symbol, position.size);

        let price = self
            .candles
            .get(&position.symbol)
            .ok_or(PrepareError::MarketClosed)?
            .map(|candle| candle.close)
            .ok_or(PrepareError::MarketClosed)?;
        let quote_size = rounded_size * price;
        /*
        let estimated_price = self
            .markets
            .market(position.market)
            .unwrap()
            .orderbook()
            .execution_price(position.size, position.side)
            .unwrap();
        */

        match position.symbol {
            Symbol::Perp(_) => {
                self.wallet
                    .reserve(quote_size, self.api.quote_asset())
                    .map_err(|_| PrepareError::InsufficientAssets)?;
            }
        }

        

        Ok(PreparedPosition {
            market: position.symbol,
            side: position.side,
            size: position.size,
            //estimated_price,
            rounded_size,
            price,
            time: self.current_time,
            id: PositionId(Uuid::new_v4()),
        })
    }

    pub fn enter(&mut self, position: PreparedPosition) {
        self.prepared_positions.push(position);
    }

    pub fn exit(&self, position: &OpenPosition) {
        self.to_close.borrow_mut().insert(position.id());
    }

    /// Iterate through all prepared positions and modify them.
    pub fn prepared_positions(&self) -> impl Iterator<Item = &PreparedPosition> {
        self.prepared_positions.iter()
    }

    /// Iterate through all open positions and modify them.
    pub fn open_positions(&self) -> impl Iterator<Item = &OpenPosition> {
        self.open_positions.iter()
    }

    /// Iterate through all closed positions and modify them.
    pub fn closed_positions(&self) -> impl Iterator<Item = &ClosedPosition> {
        self.closed_positions.iter()
    }

    /// Get wallet.
    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    pub fn round_size(&self, symbol: Symbol, size: Decimal) -> Decimal {
        let increment = self.markets.market(symbol).unwrap().size_increment;
        if increment.is_zero() {
            size
        } else {
            (size / increment).round_dp_with_strategy(0, RoundingStrategy::ToPositiveInfinity)
                * increment
        }
    }

    pub fn round_price(&self, symbol: Symbol, price: Decimal) -> Decimal {
        let increment = self.markets.market(symbol).unwrap().price_increment;
        if increment.is_zero() {
            price
        } else {
            (price / increment).round_dp_with_strategy(0, RoundingStrategy::ToPositiveInfinity)
                * increment
        }
    }

    // Run the strategy until a non-recoverable error occurs.
    async fn run_internal<S>(&mut self, strategy: &mut S, options: &Options) -> Result<(), AnyError>
    where
        S: Strategy<A>,
    {
        loop {
            // Duration to wait until next candle is available,
            // if less than zero, the candle should be available.
            let mut wait_duration = self.current_time + options.interval - Utc::now();
            if wait_duration <= Duration::zero() {
                // Update wallet and market info.
                try_join!(
                    async {
                        log::trace!("Update markets.");
                        self.api.update_markets(&mut self.markets).await?;
                        Ok::<(), AnyError>(())
                    },
                    async {
                        log::trace!("Update markets.");
                        self.api.update_wallet(&mut self.wallet).await?;
                        Ok::<(), AnyError>(())
                    },
                    async {
                        log::trace!("Update candles.");
                        let mut candles_missing: Vec<Symbol> =
                            self.candles.keys().cloned().collect();

                        // While the next candle is not already available
                        // and we don't have all candles, fetch candles.
                        while !candles_missing.is_empty() && wait_duration <= -options.interval {
                            log::trace!("Some candles are missing, fetching them.");
                            // Fetch all candles concurrently.
                            let mut futures = Vec::new();
                            for &market in candles_missing.iter() {
                                futures.push(self.api.get_candle(CandleKey {
                                    market,
                                    time: self.current_time,
                                    interval: options.interval,
                                }));
                            }
                            let candles = join_all(futures).await;
                            for (asset, candle) in candles_missing.iter().zip(candles) {
                                self.candles.insert(*asset, candle?);
                            }

                            // https://doc.rust-lang.org/std/vec/struct.Vec.html#method.drain_filter.
                            let mut i = 0;
                            while i < candles_missing.len() {
                                if self.candles.contains_key(&candles_missing[i]) {
                                    candles_missing.remove(i);
                                } else {
                                    i += 1;
                                }
                            }

                            if !candles_missing.is_empty() {
                                log::trace!("Waiting for new candles.");
                                // There still are some candles that could not be fetched.
                                // Wait a bit and try again.
                                wait_duration = self.current_time + options.interval - Utc::now();
                                tokio::time::sleep(
                                    Duration::seconds(1).to_std().expect("Converting to std"),
                                )
                                .await;
                            }
                        }

                        Ok::<(), AnyError>(())
                    }
                )?;

                // Evaluate strategy and handle errors.
                log::info!("Running strategy for time {}.", self.current_time);
                strategy.eval(self)?;
                log::trace!("Exiting positions.");
                self.exit_many().await?;
                log::trace!("Entering positions.");
                self.enter_many().await?;
                self.step(&options);
            } else {
                // Wait until next candles should be available.
                self.real_time = true;
                tokio::time::sleep(wait_duration.to_std().expect("Converting to std")).await;
            }
        }
    }

    fn step(&mut self, options: &Options) {
        self.current_time = self.current_time + options.interval;
        for candle in self.candles.values_mut() {
            *candle = None;
        }
    }

    /// Start running a strategy on an exchange.
    pub async fn run<S>(mut self, mut strategy: S) -> Result<(), AnyError>
    where
        S: Strategy<A>,
    {
        try_join!(
            async {
                log::trace!("Update markets.");
                self.api.update_markets(&mut self.markets).await?;
                Ok::<(), AnyError>(())
            },
            async {
                log::trace!("Update markets.");
                self.api.update_wallet(&mut self.wallet).await?;
                Ok::<(), AnyError>(())
            },
        )?;
        let options = strategy.init(&mut self)?;

        if A::LIVE_TRADING_ENABLED && !options.live_trading {
            log::warn!("This strategy does not support live trading.");
            return Ok(());
        }

        loop {
            match self.run_internal(&mut strategy, &options).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    log::error!("An error occured: {}", err);
                    match options.on_error {
                        OnError::Return => {
                            Err(err)?;
                        }
                        OnError::ExitAllPositionsAndReturn => {
                            for position in self.open_positions.iter() {
                                self.exit(position);
                            }
                            self.exit_many().await?;
                            Err(err)?;
                        }
                        OnError::ExitAllPositionsAndResume => {
                            for position in self.open_positions.iter() {
                                self.exit(position);
                            }
                            self.exit_many().await?;
                            // Go to next step and try again.
                            self.step(&options);
                        }
                    }
                }
            }
        }
    }

    async fn enter_many(&mut self) -> Result<(), ApiError> {
        let mut order_futures = Vec::new();
        let to_enter: Vec<PreparedPosition> = self.prepared_positions.drain(..).collect();
        for prepared_position in to_enter {
            order_futures.push(self.enter_one(prepared_position));
        }
        for order_result in join_all(order_futures).await {
            let open_position = order_result?;
            let quote_enter_size = open_position.enter_size * open_position.enter_price;
            //let quote_reserved_size = open_position.rounded_size * open_position.price;

            match open_position.market {
                Symbol::Perp(_) => {
                    self.wallet
                        .withdraw(quote_enter_size, self.api.quote_asset())
                        .unwrap();
                    /*
                    self.wallet
                        .free(
                            quote_reserved_size - quote_enter_size,
                            self.api.quote_asset(),
                        )
                        .unwrap();
                    */
                }
            }

            self.open_positions.push(open_position);
        }
        /*
        for prepared_position in to_discard {
            let quote_rounded_size = prepared_position.rounded_size * prepared_position.price;

            match prepared_position.market {
                Symbol::Perp(_) => {
                    self.wallet
                        .free(quote_rounded_size, self.api.quote_asset())
                        .unwrap();
                }
            }
        }
        */
        self.wallet
            .free_all(self.api.quote_asset());

        Ok(())
    }

    async fn exit_many(&mut self) -> Result<(), ApiError> {
        let mut order_futures = Vec::new();
        let (to_exit, to_keep): (Vec<OpenPosition>, Vec<OpenPosition>) = self
            .open_positions
            .drain(..)
            .partition(|open_position| self.to_close.borrow_mut().remove(&open_position.id()));
        for open_position in to_exit {
            order_futures.push(self.exit_one(open_position));
        }
        for order_result in join_all(order_futures).await {
            let closed_position = order_result?;
            let quote_exit_size = closed_position.exit_size * closed_position.exit_price;

            match closed_position.market {
                Symbol::Perp(_) => {
                    self.wallet.deposit(quote_exit_size, self.api.quote_asset());
                }
            }

            self.closed_positions.push(closed_position);
        }
        self.open_positions = to_keep;

        Ok(())
    }

    async fn enter_one(&self, prepared_position: PreparedPosition) -> Result<OpenPosition, ApiError> {
        let update = self
            .api
            .place_order(Order {
                market: prepared_position.market,
                side: prepared_position.side,
                size: prepared_position.size,
                order_type: OrderType::Market,
                reduce_only: false,
                time: self.current_time,
            })
            .await?;

        Ok(OpenPosition {
            market: prepared_position.market,
            side: prepared_position.side,
            size: prepared_position.size,
            price: prepared_position.price,
            //estimated_price: prepared_position.estimated_price,
            rounded_size: prepared_position.rounded_size,
            enter_price: update.price,
            enter_size: update.size,
            time: prepared_position.time,
            enter_time: update.time,
            id: prepared_position.id,
        })
    }

    async fn exit_one(&self, open_position: OpenPosition) -> Result<ClosedPosition, ApiError> {
        let update = self
            .api
            .place_order(Order {
                market: open_position.market,
                side: open_position.side.other(),
                size: open_position.enter_size,
                order_type: OrderType::Market,
                reduce_only: true,
                time: self.current_time,
            })
            .await?;

        Ok(ClosedPosition {
            market: open_position.market,
            side: open_position.side,
            size: open_position.size,
            price: open_position.price,
            //estimated_price: open_position.estimated_price,
            rounded_size: open_position.rounded_size,
            enter_price: open_position.enter_price,
            enter_size: open_position.enter_size,
            exit_price: update.price,
            exit_size: update.size,
            time: open_position.time,
            enter_time: open_position.enter_time,
            exit_time: update.time,
            id: open_position.id,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PositionId(Uuid);

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Position {
    pub symbol: Symbol,
    pub side: Side,
    /// Position size expressed in the base asset.
    pub size: Decimal,
}

pub trait PositionData {
    fn symbol(&self) -> Symbol;
    fn id(&self) -> PositionId;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PreparedPosition {
    market: Symbol,
    side: Side,
    size: Decimal,
    price: Decimal,
    //estimated_price: Decimal,
    rounded_size: Decimal,
    time: DateTime<Utc>,
    id: PositionId,
}

impl PositionData for PreparedPosition {
    fn symbol(&self) -> Symbol {
        self.market
    }

    fn id(&self) -> PositionId {
        self.id
    }
}

impl PreparedPosition {
    /*
    /// Returns the estimated execution price including slippage.
    pub fn estimated_price(&self) -> Decimal {
        self.estimated_price
    }
    */

    /// Returns the actual size after rounding.
    pub fn rounded_size(&self) -> Decimal {
        self.rounded_size
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct OpenPosition {
    market: Symbol,
    side: Side,
    size: Decimal,
    price: Decimal,
    //estimated_price: Decimal,
    rounded_size: Decimal,
    enter_price: Decimal,
    enter_size: Decimal,
    time: DateTime<Utc>,
    enter_time: DateTime<Utc>,
    id: PositionId,
}

impl PositionData for OpenPosition {
    fn symbol(&self) -> Symbol {
        self.market
    }

    fn id(&self) -> PositionId {
        self.id
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ClosedPosition {
    market: Symbol,
    side: Side,
    size: Decimal,
    price: Decimal,
    //estimated_price: Decimal,
    rounded_size: Decimal,
    enter_price: Decimal,
    enter_size: Decimal,
    exit_price: Decimal,
    exit_size: Decimal,
    time: DateTime<Utc>,
    enter_time: DateTime<Utc>,
    exit_time: DateTime<Utc>,
    id: PositionId,
}

impl PositionData for ClosedPosition {
    fn symbol(&self) -> Symbol {
        self.market
    }

    fn id(&self) -> PositionId {
        self.id
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn other(&self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}
