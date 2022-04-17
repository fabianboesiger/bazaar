use std::collections::{HashMap, VecDeque};

use super::Wallet;
use crate::{
    apis::{Api, ApiError},
    strategies::{OnError, Settings, Strategy},
    Candle, CandleKey, MarketInfo, Markets, Order, OrderType, Symbol,
};
use crate::{OrderInfo, Side, WalletError};
use chrono::{DateTime, Duration, Utc};
use futures_util::{future::join_all, try_join};
use rust_decimal::prelude::*;
use uuid::Uuid;

pub type AnyError = Box<dyn std::error::Error>;
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
    api: A,
    // Keeps track of the wallet.
    wallet: Wallet,
    // Current candles of all subscribed tickers.
    // TODO: Add this to markets?
    candles: HashMap<Symbol, VecDeque<(CandleKey, Option<Candle>)>>,
    markets: Markets,
    current_time: DateTime<Utc>,
    real_time: bool,
    open_positions_prev: Vec<Position>,
    open_positions: Vec<Position>,
}

impl<A: Api> Exchange<A> {
    /// Create an exchange layer using the specified API.
    pub fn new(api: A, start_time: DateTime<Utc>) -> Self {
        Exchange {
            current_time: start_time,
            wallet: Wallet::new(),
            //open_positions: Vec::new(),
            //closed_positions: Vec::new(),
            candles: HashMap::new(),
            markets: Markets::new(),
            api,
            real_time: false,
            open_positions_prev: Vec::new(),
            open_positions: Vec::new(),
        }
    }

    pub fn current_time(&self) -> DateTime<Utc> {
        self.current_time
    }

    pub fn is_real_time(&self) -> bool {
        self.real_time
    }

    /// List all available markets.
    pub fn markets(&self) -> impl Iterator<Item = &MarketInfo> {
        self.markets.markets().map(|(_, info)| info)
    }

    pub fn market(&self, symbol: Symbol) -> &MarketInfo {
        self.markets.market(symbol).unwrap()
    }

    /// Fetch the current candle of a market.
    pub fn candle(&self, market: Symbol) -> Option<&Candle> {
        let front = self.candles.get(&market)?.front()?;
        assert_eq!(front.0.time, self.current_time);
        front.1.as_ref()
    }

    // Fetch the current price for a market.
    pub fn price(&self, market: Symbol) -> Option<Decimal> {
        self.candle(market).map(|candle| candle.close)
    }
    /// Begin watching a market.
    pub fn watch(&mut self, market: Symbol) {
        self.candles.insert(market, VecDeque::new());
    }

    /// Stop watching a market.
    pub fn unwatch(&mut self, market: Symbol) {
        self.candles.remove(&market);
    }

    /// Enter a new position.
    pub fn open(&mut self, position: Position) -> Result<&Position, WalletError> {
        let rounded_position = Position {
            position_id: position.position_id,
            sizes: position
                .sizes
                .into_iter()
                .map(|(symbol, (size, entry_price))| {
                    (symbol, (self.round_size(symbol, size), entry_price))
                })
                .collect(),
        };

        self.open_positions.push(rounded_position);

        Ok(self.open_positions.last().unwrap())
    }

    /*
    pub fn close(&mut self, position: &Position) {
        let mut quote_size = Decimal::ZERO;
        for (symbol, size) in &position.sizes {
            quote_size += size.abs() * self.candle(*symbol).unwrap().close;
        }
        self.checker_wallet
            .deposit(quote_size, self.api.quote_asset());

        position.close();
    }

    pub fn close_all(&mut self) {
        for position in &self.open_positions {
            self.close(position);
        }
    }
    */

    /// Mutate your already open positions.
    pub fn positions(&mut self) -> impl Iterator<Item = &mut Position> {
        self.open_positions.iter_mut()
    }

    pub fn close_all(&mut self) {
        for position in self.positions() {
            position.close();
        }
    }

    /// Get wallet.
    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    pub fn total(&self) -> Decimal {
        let wallet_total = self.wallet.total(self.api.quote_asset());
        let positions_total: Decimal = self
            .open_positions
            .iter()
            .map(|position| position.total_value(self))
            .sum();

        wallet_total + positions_total
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
    async fn run_internal<S>(
        &mut self,
        strategy: &mut S,
        settings: &Settings,
    ) -> Result<(), AnyError>
    where
        S: Strategy<A>,
    {
        loop {
            // Duration to wait until next candle is available,
            // if less than zero, the candle should be available.
            let mut wait_duration = self.current_time + settings.interval - Utc::now();
            if wait_duration <= Duration::zero() {
                // Update wallet and market info.
                self.update(settings, &mut wait_duration).await?;

                strategy.eval(self)?;
                /*
                log::trace!("Exiting positions.");
                self.exit_many().await?;
                log::trace!("Entering positions.");
                self.enter_many().await?;
                */
                self.execute().await?;

                // Evaluate strategy and handle errors.
                log::info!(
                    "Ran strategy for time {}, new total value: {}",
                    self.current_time,
                    self.total()
                );

                self.api.status(self.current_time, self.total());
                self.step(settings);
            } else {
                /*
                for (_, candles) in &self.candles {
                    assert!(candles.is_empty(), "{:?}", candles);
                }
                */
                log::trace!("Waiting {} for new candles.", wait_duration);
                // Wait until next candles should be available.
                self.real_time = true;
                tokio::time::sleep(wait_duration.to_std().expect("Converting to std")).await;
            }
        }
    }

    fn step(&mut self, settings: &Settings) {
        log::trace!("Advancing time!");
        self.current_time = self.current_time + settings.interval;
        for candles in self.candles.values_mut() {
            candles.pop_front();
        }
    }

    async fn update(
        &mut self,
        settings: &Settings,
        wait_duration: &mut Duration,
    ) -> Result<(), AnyError> {
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
                let mut candles_missing: Vec<Symbol> = self
                    .candles
                    .iter()
                    .filter(|(_asset, candles)| candles.is_empty() || candles.front().is_none())
                    .map(|(asset, _)| *asset)
                    .collect();

                // While the next candle is not already available
                // and we don't have all candles, fetch candles.
                while !candles_missing.is_empty() {
                    log::trace!("Some candles are missing, fetching them.");
                    // Fetch all candles concurrently.
                    let mut futures = Vec::new();
                    for &market in candles_missing.iter() {
                        futures.push(self.api.get_candles(CandleKey {
                            market,
                            time: self.current_time,
                            interval: settings.interval,
                        }));
                    }
                    let candles = join_all(futures).await;
                    for (asset, new_candles) in candles_missing.iter().zip(candles) {
                        if let Some(candles) = self.candles.get_mut(asset) {
                            candles.append(&mut VecDeque::from_iter(new_candles?.into_iter()));
                        }
                    }

                    // https://doc.rust-lang.org/std/vec/struct.Vec.html#method.drain_filter.
                    let mut i = 0;
                    while i < candles_missing.len() {
                        // Remove present candles from missing list.
                        if self.candles.contains_key(&candles_missing[i])
                            && self
                                .candles
                                .get(&candles_missing[i])
                                .unwrap()
                                .front()
                                .is_some()
                        {
                            candles_missing.remove(i);
                        } else {
                            i += 1;
                        }
                    }

                    if *wait_duration <= -settings.interval {
                        log::trace!("Stop waiting for new candles.");
                        break;
                    } else if !candles_missing.is_empty() {
                        log::trace!("Waiting for new candles.");
                        // There still are some candles that could not be fetched.
                        // Wait a bit and try again.
                        tokio::time::sleep(
                            Duration::seconds(3).to_std().expect("Converting to std"),
                        )
                        .await;
                        *wait_duration = self.current_time + settings.interval - Utc::now();
                    }
                }

                Ok::<(), AnyError>(())
            }
        )?;

        Ok(())
    }

    /// Start running a strategy on an exchange.
    pub async fn run<S>(mut self, mut strategy: S) -> Result<(), AnyError>
    where
        S: Strategy<A>,
    {
        self.api.hello(S::NAME);

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

        if A::LIVE_TRADING_ENABLED {
            log::warn!("Trading live on exchange!");
        }

        loop {
            match self.run_internal(&mut strategy, &options).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    log::error!("An error occured: {}", err);
                    match options.on_error {
                        OnError::Return => {
                            return Err(err);
                        }
                        OnError::ExitAllPositionsAndReturn => {
                            self.close_all();
                            self.execute().await?;

                            return Err(err);
                        }
                        OnError::ExitAllPositionsAndResume => {
                            self.close_all();
                            self.execute().await?;

                            // Go to next step and try again.
                            self.step(&options);
                        }
                    }
                }
            }
        }
    }

    async fn execute(&mut self) -> Result<(), ApiError> {
        /*
        self.exit_many().await?;
        self.enter_many().await?;
        */

        // Compute diff between current wanted positions and previous positions.
        let mut curr: HashMap<Symbol, Decimal> = HashMap::new();
        let mut next: HashMap<Symbol, Decimal> = HashMap::new();
        let mut diff: HashMap<Symbol, Decimal> = HashMap::new();
        for position in &self.open_positions {
            for (symbol, (size, _)) in &position.sizes {
                *diff.entry(*symbol).or_default() += size;
                *next.entry(*symbol).or_default() += size;
            }
        }
        for position in &self.open_positions_prev {
            for (symbol, (size, _)) in &position.sizes {
                *diff.entry(*symbol).or_default() -= size;
                *curr.entry(*symbol).or_default() += size;
            }
        }

        // Compute orders from diff.
        let mut orders = Vec::new();
        for (&symbol, &size) in &diff {
            if size != Decimal::ZERO {
                //log::error!("diff: {}, {}", symbol, size);

                // 0 <= curr <= next ==> qty = next - curr
                // next <= curr <= 0 ==> qty = (-next) - (-curr)
                // curr <= 0 <= next & -curr <= next ==> next - (-curr)
                // curr <= 0 <= next & next <= -curr ==> next - (-curr)

                orders.push(Order {
                    order_id: Uuid::new_v4(),
                    market: symbol,
                    side: if size > Decimal::ZERO {
                        Side::Buy
                    } else {
                        Side::Sell
                    },
                    size: size.abs(),
                    order_type: OrderType::Market,
                    reduce_only: next.get(&symbol).cloned().unwrap_or_default().is_zero(),
                    time: self.current_time,
                    current_price: self.price(symbol).unwrap(),
                })
            }
        }

        // Execute orders, first orders that decrease position sizes, then orders that increase position sizes.
        let (decrease_position_orders, increase_position_orders) =
            orders.into_iter().partition(|order| {
                next.get(&order.market).cloned().unwrap_or_default().abs()
                    < curr.get(&order.market).cloned().unwrap_or_default().abs()
            });

        let mut order_infos = Vec::new();
        order_infos.append(&mut self.order(decrease_position_orders).await?);
        order_infos.append(&mut self.order(increase_position_orders).await?);

        let mut modified_open_positions = self.open_positions.clone();

        // Update actual positions based on order infos.
        for order_info in order_infos {
            let want = diff
                .get(&order_info.market)
                .cloned()
                .unwrap_or_default()
                .abs();
            let got = order_info.size;

            // Iterate all relevant position sizes and their previous size.
            /*
            for (value, (size, entry_price), (size_prev, entry_price_prev)) in self.open_positions.iter_mut().filter_map(|position| {
                let market = order_info.market;
                let size = position.sizes.get_mut(&market)?;

                let size_prev = self
                    .open_positions_prev
                    .iter()
                    .find(|position_prev| position_prev.position_id == position.position_id)
                    .map(|position_prev| {
                        position_prev
                            .sizes
                            .get(&market)
                            .cloned()
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();

                Some((position.value(&self, order_info.market), size, size_prev))
            }) {
            */

            for position in &mut modified_open_positions {
                if let Some((size, entry_price)) = position.sizes.get_mut(&order_info.market) {
                    let default = Position::new();
                    let position_prev = self
                        .open_positions_prev
                        .iter()
                        .find(|position_prev| position_prev.position_id == position.position_id)
                        .unwrap_or(&default);
                    let (size_prev, entry_price_prev) = position_prev
                        .sizes
                        .get(&order_info.market)
                        .cloned()
                        .unwrap_or_default();

                    // position_prev + diff * 1 = position
                    // position_prev + diff * (got / want) = position_corrected
                    // position_prev + (position - position_prev) * (got / want) = position_corrected

                    // Adapt position size.

                    let diff = *size - size_prev;
                    if diff != Decimal::ZERO {
                        let diff_actual = diff * got / want;
                        *size = size_prev + diff_actual;
                    }

                    let diff = size.abs() - size_prev.abs(); // positive -> position increase, negative -> position decrease

                    // Compute updated entry price.
                    if size.abs() > Decimal::ZERO {
                        if size.signum() != size_prev.signum() {
                            *entry_price = order_info.price;
                        } else {
                            *entry_price = (entry_price_prev * size_prev.abs()
                                + order_info.price * diff.abs())
                                / size.abs();
                        }
                    }

                    if diff > Decimal::ZERO {
                        // Increase position size.
                        let value = position.value(self, order_info.market);
                        self.wallet
                            .reserve(value, self.api.quote_asset())
                            .expect("Reserve quote asset");
                        self.wallet
                            .withdraw(value, self.api.quote_asset())
                            .expect("Withdraw quote asset");
                    } else if diff < Decimal::ZERO {
                        // Decrease position size.
                        let value = position_prev.value(self, order_info.market);
                        self.wallet.deposit(value, self.api.quote_asset());
                    }

                    //log::error!("Entry price: {}, {}, {}, {}, {}", size, size_prev, entry_price, entry_price_prev, order_info.price);
                }
            }
        }

        // Clear all closed positions.
        modified_open_positions
            .retain(|position| position.sizes.iter().any(|(_, (size, _))| !size.is_zero()));
        self.open_positions = modified_open_positions.clone();
        self.open_positions_prev = modified_open_positions.clone();

        //log::error!("actual open positions: {:?}", self.open_positions);

        Ok(())
    }

    async fn order(&self, orders: Vec<Order>) -> Result<Vec<OrderInfo>, ApiError> {
        let mut order_futures = Vec::new();
        for order in orders {
            order_futures.push(self.api.place_order(order));
        }
        join_all(order_futures).await.into_iter().collect()
    }
}

#[derive(Clone, Debug)]
pub struct Position {
    position_id: Uuid,
    sizes: HashMap<Symbol, (Decimal, Decimal)>,
}

impl Position {
    pub fn new() -> Self {
        Self {
            position_id: Uuid::new_v4(),
            sizes: HashMap::new(),
        }
    }

    pub fn size(&mut self, symbol: Symbol) -> &mut Decimal {
        &mut self.sizes.entry(symbol).or_default().0
    }

    #[cfg(test)]
    fn entry_price(&mut self, symbol: Symbol) -> &mut Decimal {
        &mut self.sizes.entry(symbol).or_default().1
    }

    pub fn value<A: Api>(&self, exchange: &Exchange<A>, symbol: Symbol) -> Decimal {
        let (size, entry_price) = self.sizes.get(&symbol).cloned().unwrap_or_default();
        assert!(entry_price >= Decimal::ZERO);
        let open_value = entry_price * size.abs();
        let close_value = exchange.price(symbol).unwrap() * size.abs();
        assert!(open_value >= Decimal::ZERO);
        assert!(close_value >= Decimal::ZERO);
        let pnl = if size >= Decimal::ZERO {
            close_value - open_value
        } else {
            open_value - close_value
        };
        let result = open_value + pnl;
        assert!(result >= Decimal::ZERO);
        result
    }

    pub fn total_value<A: Api>(&self, exchange: &Exchange<A>) -> Decimal {
        self.sizes
            .keys()
            .map(|&symbol| self.value(exchange, symbol))
            .sum()
    }

    pub fn close(&mut self) {
        for (size, _) in self.sizes.values_mut() {
            *size = Decimal::ZERO;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::apis::Ftx;
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn long_position() {
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();
        let exchange = Exchange {
            api: Ftx::new(),
            wallet: Wallet::new(),
            candles: {
                let mut candles = HashMap::new();
                let mut vec = VecDeque::new();
                vec.push_back((
                    CandleKey {
                        market: symbol,
                        time,
                        interval: Duration::minutes(1),
                    },
                    Some(Candle {
                        close: dec!(20000),
                        volume: dec!(0),
                    }),
                ));
                candles.insert(symbol, vec);
                candles
            },
            markets: Markets::new(),
            current_time: time,
            real_time: false,
            open_positions_prev: Vec::new(),
            open_positions: Vec::new(),
        };

        let mut position = Position::new();
        *position.size(symbol) = dec!(1);
        *position.entry_price(symbol) = dec!(10000);
        assert_eq!(position.total_value(&exchange), dec!(20000));
    }

    #[test]
    fn short_position() {
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();
        let exchange = Exchange {
            api: Ftx::new(),
            wallet: Wallet::new(),
            candles: {
                let mut candles = HashMap::new();
                let mut vec = VecDeque::new();
                vec.push_back((
                    CandleKey {
                        market: symbol,
                        time,
                        interval: Duration::minutes(1),
                    },
                    Some(Candle {
                        close: dec!(20000),
                        volume: dec!(0),
                    }),
                ));
                candles.insert(symbol, vec);
                candles
            },
            markets: Markets::new(),
            current_time: time,
            real_time: false,
            open_positions_prev: Vec::new(),
            open_positions: Vec::new(),
        };

        let mut position = Position::new();
        *position.size(symbol) = dec!(-1);
        *position.entry_price(symbol) = dec!(10000);
        assert_eq!(position.total_value(&exchange), dec!(0));
    }

    /*
    #[tokio::test]
    async fn enter_exit_position() {
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();
        let mut wallet = Wallet::new();
        let quote_asset = Asset::new("USD");
        wallet.deposit(dec!(10000), quote_asset);

        let api = Simulate::new(Ftx::new(), wallet);
        let mut exchange = Exchange {
            api,
            wallet: Wallet::new(),
            candles: {
                let mut candles = HashMap::new();
                let mut vec = VecDeque::new();
                vec.push_back((CandleKey {
                    market: symbol,
                    time,
                    interval: Duration::minutes(1),
                }, Some(Candle {
                    close: dec!(10000),
                    volume: dec!(0),
                })));
                candles.insert(symbol, vec);
                candles
            },
            markets: Markets::new(),
            current_time: time,
            real_time: false,
            open_positions_prev: Vec::new(),
            open_positions: Vec::new(),
        };

        let mut position = Position::new();
        *position.size(symbol) = dec!(1);

        exchange.open(position).unwrap();
        exchange.execute().await.unwrap();

        assert_eq!(exchange.wallet().total(quote_asset), dec!(0));
    }
    */
}
