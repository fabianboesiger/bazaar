mod bundle;
mod position;
mod valuation;
mod valued_bundle;

use bundle::Bundle;
pub use position::Position;
use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
};
use valuation::Valuation;
use valued_bundle::ValuedBundle;

use super::Wallet;
use crate::{
    apis::{Api, ApiError},
    strategies::{OnError, Settings, Strategy},
    Candle, CandleKey, MarketInfo, Markets, Order, Symbol,
};
use crate::{OrderInfo, Side, WalletError};
use chrono::{DateTime, Duration, Utc};
use futures_util::{future::join_all, try_join};
use rust_decimal::prelude::*;

pub type AnyError = Box<dyn std::error::Error>;
use thiserror::Error;

type Candles = HashMap<Symbol, VecDeque<(CandleKey, Option<Candle>)>>;

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
    candles: Candles,
    markets: Markets,
    current_time: DateTime<Utc>,
    real_time: bool,
    open_positions: Vec<Position>,
    //next_open_positions: Vec<Position>,
    debug_msg: Option<Box<dyn Debug>>,
    quit: bool,
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
            markets: Markets::default(),
            api,
            real_time: false,
            open_positions: Vec::new(),
            //next_open_positions: Vec::new(),
            debug_msg: None,
            quit: false,
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

    /// Quit trading.,
    pub fn quit(&mut self) {
        self.quit = true;
    }

    /// Enter a new position.
    pub fn open(&mut self, mut position: Position) -> Result<&Position, WalletError> {
        position.fit(self);
        self.open_positions.push(position);
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

    pub fn debug<D: Debug + 'static>(&mut self, msg: D) {
        self.debug_msg = Some(Box::new(msg));
    }

    /// Mutate your already open positions.
    pub fn positions_mut(&mut self) -> impl Iterator<Item = &mut Position> {
        self.open_positions.iter_mut()
    }

    /// Iterate your already open positions.
    pub fn positions(&self) -> impl Iterator<Item = &Position> {
        self.open_positions.iter()
    }

    pub fn close_all(&mut self) {
        for position in self.positions_mut() {
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
            .map(|position| position.value())
            .sum();

        wallet_total + positions_total
    }
    /*
    pub fn round_size(&self, symbol: Symbol, size: Decimal) -> Decimal {
        let increment = self.markets.market(symbol).unwrap().size_increment;
        if increment.is_zero() {
            size
        } else {
            (size / increment).round()
                * increment
        }
    }

    pub fn round_price(&self, symbol: Symbol, price: Decimal) -> Decimal {
        let increment = self.markets.market(symbol).unwrap().price_increment;
        if increment.is_zero() {
            price
        } else {
            (price / increment).round()
                * increment
        }
    }
    */

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
                // Update position value.
                self.valuate();

                strategy.eval(self)?;

                // Update position value again for potential new positions.
                self.valuate();

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

    fn valuate(&mut self) {
        let valuation = Valuation(
            self.candles
                .iter()
                .filter_map(|(&symbol, candle)| Some((symbol, candle.front()?.1?.close)))
                .collect(),
        );

        let time = self.current_time();

        for position in self.positions_mut() {
            position.valuate(valuation.clone(), time);
        }
    }

    async fn execute(&mut self) -> Result<(), ApiError> {
        // Get all orders.
        let orders: Vec<ValuedBundle> = self.positions().map(|position| position.order()).collect();
        for order in &orders {
            assert!(order.time.is_some());
        }

        // Order and get order results.
        let order_results = self.order(orders).await?;

        let mut value_diff_sum = Decimal::ZERO;
        for (position, order_result) in self.positions_mut().zip(order_results) {
            let before_value = position.value();

            // Adapt positions to order results.
            position.resize(order_result);

            // Change wallet value.
            let after_value = position.value();
            let value_diff = after_value - before_value;
            //println!("before: {}, after: {}", before_value, after_value);
            value_diff_sum += value_diff;
        }


        if value_diff_sum > Decimal::ZERO {
            println!("withdraw {}", value_diff_sum);
            self.wallet.reserve(value_diff_sum, self.api.quote_asset()).unwrap();
            self.wallet.withdraw(value_diff_sum, self.api.quote_asset()).unwrap();
        } else if value_diff_sum < Decimal::ZERO {
            println!("deposit {}", value_diff_sum);
            self.wallet.deposit(-value_diff_sum, self.api.quote_asset());
        }

        // Remove closed positions.
        self.open_positions
            .retain(|position| position.value() != Decimal::ZERO);

        Ok(())
    }

    async fn order(&self, orders: Vec<ValuedBundle>) -> Result<Vec<ValuedBundle>, ApiError> {
        // Coalesce orders to issue only one order per symbol.
        let actual_orders: Vec<Order> = Self::coalesce_orders(&orders).into();
        let mut actual_order_futures = Vec::new();
        for actual_order in actual_orders.iter() {
            actual_order_futures.push(self.api.place_order(actual_order.clone()));
        }
        let actual_order_results: Result<Vec<OrderInfo>, ApiError> =
            join_all(actual_order_futures).await.into_iter().collect();
        let actual_order_results = actual_order_results?;

        let mut adjusted_orders = orders.clone();
        for (actual_order, actual_order_result) in
            actual_orders.iter().zip(actual_order_results.iter())
        {
            assert_eq!(actual_order.market, actual_order_result.market);
            assert_eq!(actual_order.side, actual_order_result.side);
            let symbol = actual_order.market;
            let price = actual_order_result.price;

            let missing = if actual_order.side == Side::Buy {
                actual_order.size - actual_order_result.size
            } else {
                -(actual_order.size - actual_order_result.size)
            };

            //println!("order: {}, price: {}, missing: {}", symbol, price, missing);

            let same_side_order_size_sum: Decimal = orders
                .iter()
                .filter_map(|order| order.bundle.0.get(&symbol).cloned())
                .filter(|order| order.signum() == missing.signum())
                .sum();

            for (adjusted_order, order_size) in adjusted_orders
                .iter_mut()
                .zip(orders.iter())
                .filter_map(|(adjusted_order, order)| {
                    Some((adjusted_order, order.bundle.0.get(&symbol).cloned()?))
                })
            {
                adjusted_order.valuation.0.insert(symbol, price);

                // Set the price from the actual order result.
                if order_size.signum() == missing.signum() {
                    //assert_ne!(same_side_order_size_sum, Decimal::ZERO);
                    if same_side_order_size_sum == Decimal::ZERO {
                        assert_eq!(order_size, Decimal::ZERO);
                    } else {
                        adjusted_order.bundle.0.insert(
                            symbol,
                            order_size - missing * order_size / same_side_order_size_sum,
                        );
                    }
                }
            }
        }

        Ok(adjusted_orders)
    }

    fn coalesce_orders(orders: &[ValuedBundle]) -> ValuedBundle {
        orders
            .iter()
            .skip(1)
            .fold(orders.first().cloned().unwrap_or_default(), |acc, vb| {
                &acc + vb
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::apis::{Ftx, Simulate};
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn coalesce_orders_none() {
        let result = Exchange::<Ftx>::coalesce_orders(&Vec::new());
        assert_eq!(result.value(), dec!(0));
    }

    #[test]
    fn coalesce_orders_single_buy() {
        let symbol = Symbol::perp("BTC");

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(10));

        let result = Exchange::<Ftx>::coalesce_orders(&[vb1]);

        assert_eq!(result.bundle.0.get(&symbol), Some(&dec!(10)));
    }

    #[test]
    fn coalesce_orders_single_sell() {
        let symbol = Symbol::perp("BTC");

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(-10));

        let result = Exchange::<Ftx>::coalesce_orders(&[vb1]);

        assert_eq!(result.bundle.0.get(&symbol), Some(&dec!(-10)));
    }

    #[test]
    fn coalesce_orders_cancel_out() {
        let symbol = Symbol::perp("BTC");

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(-10));
        let mut vb2 = ValuedBundle::default();
        vb2.bundle.0.insert(symbol, dec!(5));
        let mut vb3 = ValuedBundle::default();
        vb3.bundle.0.insert(symbol, dec!(5));

        let result = Exchange::<Ftx>::coalesce_orders(&[vb1, vb2, vb3]);

        assert_eq!(result.bundle.0.get(&symbol), Some(&dec!(0)));
    }

    #[tokio::test]
    async fn order_bundles_single_unvalued() {
        let api = Simulate::new(Ftx::from_env(), Wallet::default());
        let exchange = Exchange::new(api, Utc::now());
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(10));
        vb1.time = Some(time);

        let result = exchange.order(vec![vb1]).await.unwrap();

        assert_eq!(result[0].bundle.0.get(&symbol), Some(&dec!(10)));
    }

    #[tokio::test]
    async fn order_bundles_multiple_unvalued() {
        let api = Simulate::new(Ftx::from_env(), Wallet::default());
        let exchange = Exchange::new(api, Utc::now());
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(10));
        vb1.time = Some(time);

        let mut vb2 = ValuedBundle::default();
        vb2.bundle.0.insert(symbol, dec!(5));
        vb2.time = Some(time);

        let mut vb3 = ValuedBundle::default();
        vb3.bundle.0.insert(symbol, dec!(-15));
        vb3.time = Some(time);

        let result = exchange.order(vec![vb1, vb2, vb3]).await.unwrap();

        assert_eq!(result[0].bundle.0.get(&symbol), Some(&dec!(10)));
        assert_eq!(result[1].bundle.0.get(&symbol), Some(&dec!(5)));
        assert_eq!(result[2].bundle.0.get(&symbol), Some(&dec!(-15)));
    }

    #[tokio::test]
    async fn order_bundles_single_valued() {
        let api = Simulate::new(Ftx::from_env(), Wallet::default());
        let fee = api.order_fee().await;
        let exchange = Exchange::new(api, Utc::now());
        let symbol = Symbol::perp("BTC");
        let time = Utc::now();

        let mut vb1 = ValuedBundle::default();
        vb1.bundle.0.insert(symbol, dec!(10));
        vb1.valuation.0.insert(symbol, dec!(10000));
        vb1.time = Some(time);

        let result = exchange.order(vec![vb1]).await.unwrap();

        assert_eq!(result[0].bundle.0.get(&symbol), Some(&dec!(10)));
        assert_eq!(
            result[0].valuation.0.get(&symbol),
            Some(&(dec!(10000) * (dec!(1) + fee)))
        );
    }
}
