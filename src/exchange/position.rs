use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Bundle, Valuation, ValuedBundle};
use crate::{apis::Api, Exchange, Symbol};

#[derive(Debug, Clone)]
pub struct Position {
    id: Uuid,
    deltas: Vec<ValuedBundle>,
    /*
    current_price: Valuation,
    current_size: Bundle,
    current_time: DateTime<Utc>,
    */
    pub(crate) current: ValuedBundle,
    pub(crate) next_size: Bundle,
}

impl Default for Position {
    fn default() -> Self {
        Position {
            id: Uuid::new_v4(),
            deltas: Vec::new(),
            /*
            current_price: Valuation::default(),
            current_size: Bundle::default(),
            current_time: Utc::now(),
            */
            current: ValuedBundle {
                bundle: Bundle::default(),
                valuation: Valuation::default(),
                time: None,
            },
            next_size: Bundle::default(),
        }
    }
}

impl Position {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn long(mut self, symbol: Symbol, qty: Decimal) -> Self {
        assert!(qty >= Decimal::ZERO);
        *self.size(symbol) = qty;
        self
    }

    pub fn short(mut self, symbol: Symbol, qty: Decimal) -> Self {
        assert!(qty >= Decimal::ZERO);
        *self.size(symbol) = -qty;
        self
    }

    pub fn symbols(&self) -> impl Iterator<Item = Symbol> {
        self.current.bundle.0.keys().cloned().collect::<Vec<Symbol>>().into_iter()
    }

    // Fits this position to the exchange constrants, for example minimum order size, minimum size increment, ...
    // Returns the difference from the initial position caused by rounding as quote value.
    pub fn fit<A: Api>(&mut self, exchange: &Exchange<A>) -> Decimal {
        let mut rounded_size = self.next_size.clone();
        let order_bundle = &self.next_size - &self.current.bundle;

        // Round by size increment.
        for (&symbol, size) in &order_bundle.0 {
            let rounded_order_bundle = exchange.market(symbol).round_size(*size);
            rounded_size.0.insert(
                symbol,
                self.current
                    .bundle
                    .0
                    .get(&symbol)
                    .cloned()
                    .unwrap_or_default()
                    + rounded_order_bundle,
            );
        }

        // Round by min size requirement.
        for (&symbol, size) in &order_bundle.0 {
            let min_size = exchange.market(symbol).min_size;
            if size.abs() < min_size {
                rounded_size.0.insert(
                    symbol,
                    self.current
                        .bundle
                        .0
                        .get(&symbol)
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        }

        let rounding_diff = (&rounded_size - &self.next_size).abs();
        let rounding_value = &rounding_diff * &self.current.valuation;

        self.next_size = rounded_size;

        rounding_value
    }

    pub(crate) fn valuate(&mut self, valuation: Valuation, time: DateTime<Utc>) {
        self.current.valuation = valuation;
        self.current.time = Some(time);
    }

    /// Modify the position size.
    pub(crate) fn size(&mut self, symbol: Symbol) -> &mut Decimal {
        self.next_size.0.entry(symbol).or_default()
    }

    /// Close this position.
    pub fn close(&mut self) {
        for size in self.next_size.0.values_mut() {
            *size = Decimal::ZERO;
        }
    }

    pub(crate) fn order(&self) -> ValuedBundle {
        //let size = self.deltas.iter().map(|(bundle, _)| bundle).fold(Bundle::default(), |a, b| &a + b);
        let order_bundle = &self.next_size - &self.current.bundle;
        //assert!(self.current.time.is_some());
        ValuedBundle {
            bundle: order_bundle,
            valuation: self.current.valuation.clone(),
            time: self.current.time,
        }
    }

    pub(crate) fn resize<O: Into<ValuedBundle>>(&mut self, order: O) {
        let order: ValuedBundle = order.into();
        //self.current.valuation = order.valuation.clone();
        self.current.bundle = &self.current.bundle + &order.bundle;
        self.next_size = self.current.bundle.clone();
        self.deltas.push(order);
    }

    /*
    pub(crate) fn order(&self) -> Vec<Order> {
        let order_bundle = &self.next_size - &self.current_size;
        let mut orders = Vec::new();

        for (symbol, qty) in order_bundle.0 {
            if qty != Decimal::ZERO {
                orders.push(Order {
                    order_id: Uuid::new_v4(),
                    market: symbol,
                    side: if qty > Decimal::ZERO {
                        Side::Buy
                    } else {
                        Side::Sell
                    },
                    size: qty.abs(),
                    order_type: OrderType::Market,
                    reduce_only: self.next_size.0.get(&symbol).cloned().unwrap_or_default() == Decimal::ZERO,
                    time: self.current_time,
                    current_price: self.current.valuation.0.get(&symbol).cloned().unwrap_or_default(),
                });
            }
        }

        orders
    }
    */

    // Sum of all deltas, stepwise.
    pub(crate) fn stepwise_delta_sum(&self) -> impl Iterator<Item = Bundle> {
        self.deltas
            .iter()
            //.chain(std::iter::once(&self.current.invert()))
            .scan(Bundle::default(), |bundle_sum, curr| {
                *bundle_sum = &*bundle_sum + &curr.bundle;
                Some(bundle_sum.clone())
            })
            .collect::<Vec<Bundle>>()
            .into_iter()
    }

    // Added position value per delta.
    pub(crate) fn stepwise_added_delta_value(&self) -> impl Iterator<Item = Decimal> {
        //println!("--------------");
        self.deltas
            .iter()
            //.chain(std::iter::once(&self.current.invert()))
            .zip(
                std::iter::once(Bundle::default())
                    .chain(self.stepwise_delta_sum())
            )
            .map(|(delta, sum_bundle)| {
                let diff = &(&sum_bundle + &delta.bundle).abs() - &sum_bundle.abs();
                //println!("sum: {}", sum_bundle.0.get(&Symbol::perp("BTC")).cloned().unwrap_or_default());
                //println!("delta: {}", delta.bundle.0.get(&Symbol::perp("BTC")).cloned().unwrap_or_default());
                //println!("diff: {}", diff.0.get(&Symbol::perp("BTC")).cloned().unwrap_or_default());
                &diff * &delta.valuation
            })
            .collect::<Vec<Decimal>>()
            .into_iter()
    }

    pub(crate) fn stepwise_added_pnl_value(&self) -> impl Iterator<Item = Decimal> {
        self.deltas
            .iter()
            .zip(self.deltas
                .iter()
                .skip(1)
                .chain(std::iter::once(&self.current.invert()))
            )
            .zip(self.stepwise_delta_sum())
            .map(|((curr, next), sum_bundle)| {
                &sum_bundle * &next.valuation - &sum_bundle * &curr.valuation
            })
            .collect::<Vec<Decimal>>()
            .into_iter()

    }

    pub fn pnl(&self) -> Decimal {
        self.stepwise_added_pnl_value().sum()
    }

    pub fn value(&self) -> Decimal {
        let added_pnl_value_sum: Decimal = self.stepwise_added_pnl_value().sum();
        let added_delta_value_sum: Decimal = self.stepwise_added_delta_value().sum();
        //println!("delta: {}, pnl: {}", added_delta_value_sum, added_pnl_value_sum);
        added_pnl_value_sum + added_delta_value_sum
    }

    pub fn relative_pnl(&self) -> Decimal {
        let value_sum: Decimal = self.stepwise_added_delta_value().map(|delta| delta.abs()).sum();
        let pnl_sum: Decimal = self.stepwise_added_pnl_value().sum();
        if value_sum.is_zero() {
            Decimal::ZERO
        } else {
            assert_ne!(value_sum, Decimal::ZERO);
            pnl_sum / value_sum
        }
    }

    /* 
    pub(crate) fn iter_pnl(&self) -> impl Iterator<Item = Decimal> {
        self.iter_size()
            .zip(
                self.deltas
                    .iter()
                    .chain(std::iter::once(&self.current.invert()))
                    .zip(
                        self.deltas
                            .iter()
                            .chain(std::iter::once(&self.current.invert()))
                            .skip(1),
                    ),
            )
            .map(|(size, (curr, next))| &size * &next.valuation - &size * &curr.valuation)
            .collect::<Vec<Decimal>>()
            .into_iter()
    }

    pub(crate) fn iter_size(&self) -> impl Iterator<Item = Bundle> {
        self.deltas
            .iter()
            .scan(Bundle::default(), |bundle_sum, curr| {
                *bundle_sum = &*bundle_sum + &curr.bundle;
                Some(bundle_sum.clone())
            })
            .collect::<Vec<Bundle>>()
            .into_iter()
    }

    pub(crate) fn iter_value(&self) -> impl Iterator<Item = Decimal> {
        self.iter_size()
            .zip(self.deltas.iter().map(|delta| &delta.valuation))
            .map(|(size, valuation)| &size.abs() * valuation)
            .collect::<Vec<Decimal>>()
            .into_iter()
    }

    /// Get the current value of this position.
    pub fn value(&self) -> Decimal {
        self.iter_value()
            .zip(self.iter_pnl())
            .last()
            .map(|(value, pnl)| value + pnl)
            .unwrap_or_default()
    }

    /// Get the total profit and loss of this position.
    pub fn pnl(&self) -> Decimal {
        self.iter_pnl().sum()
    }

    /// Get the profit and loss relative to the position size.
    /// For example, a relative pnl of 0.5 would mean that this position has increased 50% in value.
    pub fn relative_pnl(&self) -> Decimal {
        let value_sum: Decimal = self.iter_value().sum();
        let pnl_sum: Decimal = self.iter_pnl().sum();
        if value_sum.is_zero() {
            Decimal::ZERO
        } else {
            pnl_sum / value_sum
        }
    }
    */


    /*
    pub fn pnl(&self) -> Decimal {
        let (bundle, value) = self.deltas
            .iter()
            .chain(std::iter::once(&self.current.invert()))
            .fold((Bundle::default(), Decimal::ZERO), |(prev_bundle, prev_value), ValuedBundle {
                bundle: curr_bundle_diff,
                valuation: curr_valuation,
                ..
            }| {
                let next_bundle = &prev_bundle + &curr_bundle_diff;
                //let position_change = &next_bundle.abs() - &prev_bundle.abs();
                let curr_value = -(curr_bundle_diff * curr_valuation);
                let next_value = prev_value + curr_value;
                (next_bundle, next_value)
            });

        //assert_eq!(bundle, self.current.bundle.buy_only());
        value
    }

    pub fn relative_pnl(&self) -> Decimal {
        let (bundle, value) = self.deltas
            .iter()
            .chain(std::iter::once(&self.current.invert()))
            .fold((Bundle::default(), Decimal::ZERO), |(prev_bundle, prev_value), ValuedBundle {
                bundle: curr_bundle_diff,
                valuation: curr_valuation,
                ..
            }| {
                let next_bundle = &prev_bundle + &curr_bundle_diff;
                let curr_value = curr_bundle_diff * curr_valuation;
                let next_value = prev_value + curr_value;
                (next_bundle, next_value)
            });

        //assert_eq!(bundle, self.current.bundle.buy_only());
        -value
    }
    */

    /*
    pub fn total_value(&self) -> Decimal {
        /*
        let (bundle, value) = self.deltas
            .iter()
            .chain(std::iter::once(&self.current))
            .fold((Bundle::default(), Decimal::ZERO), |(prev_bundle, prev_value), ValuedBundle {
                bundle: curr_bundle_diff,
                valuation: curr_valuation,
                ..
            }| {
                let next_bundle = &prev_bundle + &curr_bundle_diff;
                //let position_change = &next_bundle.abs() - &prev_bundle.abs();
                let curr_value = curr_bundle_diff * curr_valuation;
                let next_value = prev_value + curr_value;
                (next_bundle, next_value)
            });

        //assert_eq!(bundle, self.current.bundle.buy_only());
        value
        */
        println!("{} {}", self.buy_value() - self.sell_value(), self.invested());
        (self.buy_value() - self.sell_value()) + self.pnl()
    }
    */
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    /*
    #[test]
    fn position_long() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.value(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.value(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.pnl(), dec!(10000));
        assert_eq!(position.value(), dec!(20000));

        *position.size(Symbol::perp("BTC")) = dec!(0.5);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(10000));
        assert_eq!(position.value(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(5000));
        assert_eq!(position.value(), dec!(5000));

        *position.size(Symbol::perp("BTC")) = dec!(0);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(5000));
        assert_eq!(position.value(), dec!(0));
    }

    #[test]
    fn position_short() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.value(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(-1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.value(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.pnl(), dec!(5000));
        assert_eq!(position.value(), dec!(15000));

        *position.size(Symbol::perp("BTC")) = dec!(-0.5);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(5000));
        assert_eq!(position.value(), dec!(7500));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(2500));
        assert_eq!(position.value(), dec!(10000));

        *position.size(Symbol::perp("BTC")) = dec!(0);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(2500));
        assert_eq!(position.value(), dec!(5000));
    }

    #[test]
    fn position_long_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.pnl(), dec!(10000));

        *position.size(Symbol::perp("BTC")) = dec!(0.5);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(5000));

        *position.size(Symbol::perp("BTC")) = dec!(0);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(5000));
    }

    #[test]
    fn position_short_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(-1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.pnl(), dec!(5000));

        *position.size(Symbol::perp("BTC")) = dec!(-0.5);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(5000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(2500));

        *position.size(Symbol::perp("BTC")) = dec!(0);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(2500));
    }
    */

    #[test]
    fn position_simple_long_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.pnl(), dec!(10000));
    }

    #[test]
    fn position_simple_short_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(-1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.pnl(), dec!(5000));
    }

    #[test]
    fn position_simple_long_relative_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.relative_pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.relative_pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.relative_pnl(), dec!(-0.5));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.relative_pnl(), dec!(1.0));
    }

    #[test]
    fn position_simple_short_relative_pnl() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.relative_pnl(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(-1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.relative_pnl(), dec!(0));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.relative_pnl(), dec!(0.5));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.relative_pnl(), dec!(-1.0));
    }

    #[test]
    fn position_simple_long_value() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.value(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.value(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.value(), dec!(5000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.value(), dec!(20000));
    }

    #[test]
    fn position_simple_short_value() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        assert_eq!(position.value(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(-1);
        let order = position.order();
        position.resize(order);
        assert_eq!(position.value(), dec!(10000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(5000));
        assert_eq!(position.value(), dec!(15000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        assert_eq!(position.value(), dec!(0));
    }
}
