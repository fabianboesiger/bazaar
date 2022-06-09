use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Bundle, Valuation, ValuedBundle};
use crate::{apis::Api, Exchange, Symbol};

#[derive(Debug, Clone)]
pub struct Position {
    id: Uuid,
    pub(crate) current: ValuedBundle,
    pub(crate) open: Option<ValuedBundle>,
    pub(crate) close: Option<ValuedBundle>,
    pub(crate) next_size: Bundle,
}

impl Default for Position {
    fn default() -> Self {
        Position {
            id: Uuid::new_v4(),
            open: None,
            close: None,
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
        self.open
            .as_ref()
            .map(|open| {
                open.bundle
                    .0
                    .iter()
                    .filter(|(_, &qty)| qty != Decimal::ZERO)
                    .map(|(s, _)| s)
                    .cloned()
                    .collect::<Vec<Symbol>>()
                    .into_iter()
            })
            .into_iter()
            .flatten()
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
        match (&self.open, &self.close) {
            (None, None) => {
                self.open = Some(order);
            }
            (None, Some(_)) => panic!("cannot close before open"),
            (Some(_), None) => {
                self.close = Some(order);
                assert!(self.closed(), "position not fully closed");
            }
            (Some(_), Some(_)) => panic!("cannot close twice"),
        }
    }

    // Total pnl of this position.
    pub fn pnl(&self) -> Decimal {
        if let Some(close) = &self.close {
            -(self.open.as_ref().expect("open before close").value() + close.value())
        } else {
            -(self
                .open
                .as_ref()
                .map(|open| open.value())
                .unwrap_or_default()
                - self.current.value())
        }
    }

    // Total value of this position.
    pub fn value(&self) -> Decimal {
        self.open
            .as_ref()
            .map(|open| open.abs_value())
            .unwrap_or_default()
            + self.pnl()
    }

    // Profit and loss relative to the open value.
    pub fn relative_pnl(&self) -> Decimal {
        let pnl = self.pnl();
        let value = self
            .open
            .as_ref()
            .map(|open| open.abs_value())
            .unwrap_or_default();
        if value == Decimal::ZERO {
            Decimal::ZERO
        } else {
            pnl / value
        }
    }

    pub(crate) fn closed(&self) -> bool {
        let closed = self.close.is_some();
        if closed {
            assert!(self.removable());
        }
        closed
    }

    pub(crate) fn removable(&self) -> bool {
        self.next_size.0.iter().all(|(_s, qty)| *qty == Decimal::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn position_simple_neutral() {
        let mut position = Position::default();

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(10000));
        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("ETH"), dec!(1000));

        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.relative_pnl(), dec!(0));
        assert_eq!(position.value(), dec!(0));

        *position.size(Symbol::perp("BTC")) = dec!(1);
        *position.size(Symbol::perp("ETH")) = dec!(-10);

        let order = position.order();
        position.resize(order);

        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.relative_pnl(), dec!(0));
        assert_eq!(position.value(), dec!(20000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("ETH"), dec!(2000));

        assert_eq!(position.pnl(), dec!(0));
        assert_eq!(position.relative_pnl(), dec!(0));
        assert_eq!(position.value(), dec!(20000));

        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("BTC"), dec!(20000));
        position
            .current
            .valuation
            .0
            .insert(Symbol::perp("ETH"), dec!(1000));

        assert_eq!(position.pnl(), dec!(10000));
        assert_eq!(position.relative_pnl(), dec!(0.5));
        assert_eq!(position.value(), dec!(30000));
    }

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

    
    #[test]
    fn long_close() {
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

        position.close();
        let order = position.order();
        position.resize(order);

        assert_eq!(position.value(), dec!(5000));
    }

    #[test]
    fn short_close() {
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

        position.close();
        let order = position.order();
        position.resize(order);

        assert_eq!(position.value(), dec!(15000));
    }

    /* 
    #[test]
    fn close_value_to_zero() {
        for _ in 0..100 {
            let mut position = Position::default();

            for i in 0..100 {
                let symbol = Symbol::perp(&format!("{}", i));

                position.current.valuation.0.insert(
                    symbol,
                    Decimal::from_f64(rand::random::<f64>())
                        .unwrap()
                        .round_dp(8),
                );
            }

            for i in 0..100 {
                let symbol = Symbol::perp(&format!("{}", i));

                *position.size(symbol) = Decimal::from_f64(rand::random::<f64>() - 0.5)
                    .unwrap()
                    .round_dp(8);
            }

            let order = position.order();
            position.resize(order);

            for i in 0..100 {
                let symbol = Symbol::perp(&format!("{}", i));

                position.current.valuation.0.insert(
                    symbol,
                    Decimal::from_f64(rand::random::<f64>())
                        .unwrap()
                        .round_dp(8),
                );
            }

            position.close();
            let order = position.order();
            position.resize(order);

            assert_eq!(position.value(), Decimal::ZERO);
        }
    }
    */
}
