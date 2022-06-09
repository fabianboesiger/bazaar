use super::{Bundle, Valuation};
use crate::{Order, OrderInfo, OrderType, Side};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::{
    fmt::Debug,
    ops::{Add, Neg},
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct ValuedBundle {
    pub(crate) bundle: Bundle,
    pub(crate) valuation: Valuation,
    pub(crate) time: Option<DateTime<Utc>>,
}

impl ValuedBundle {
    pub fn value(&self) -> Decimal {
        &self.bundle * &self.valuation
    }

    pub fn abs_value(&self) -> Decimal {
        &self.bundle.abs() * &self.valuation
    }
}

impl From<Vec<Order>> for ValuedBundle {
    fn from(orders: Vec<Order>) -> Self {
        let mut bundle = Bundle::default();
        let mut valuation = Valuation::default();
        let mut time = None;

        for order in orders {
            bundle.0.insert(
                order.market,
                if order.side == Side::Buy {
                    order.size
                } else {
                    -order.size
                },
            );
            valuation.0.insert(order.market, order.current_price);
            if let Some(time) = time {
                assert_eq!(time, order.time);
            } else {
                time = Some(order.time);
            }
        }

        ValuedBundle {
            bundle,
            valuation,
            time,
        }
    }
}

impl From<Vec<OrderInfo>> for ValuedBundle {
    fn from(orders: Vec<OrderInfo>) -> Self {
        let mut bundle = Bundle::default();
        let mut valuation = Valuation::default();
        // TODO: How to properly deal with time?
        let mut time = None;

        for order in orders {
            bundle.0.insert(
                order.market,
                if order.side == Side::Buy {
                    order.size
                } else {
                    -order.size
                },
            );
            valuation.0.insert(order.market, order.price);
            if let Some(_time) = time {
                //assert_eq!(time, order.time);
            } else {
                time = Some(order.time);
            }
        }

        ValuedBundle {
            bundle,
            valuation,
            time,
        }
    }
}

/*
impl Into<Vec<Order>> for ValuedBundle {
    fn into(self) -> Vec<Order> {
        let mut orders = Vec::new();

        for (symbol, qty) in self.bundle.0 {
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
                    reduce_only: false,
                    time: self
                        .time
                        .expect("Cannot order valued bundle without associated time"),
                    current_price: self.valuation.0.get(&symbol).cloned().unwrap_or_default(),
                });
            }
        }

        orders
    }
}
*/

impl From<ValuedBundle> for Vec<Order> {
    fn from(valued_bundle: ValuedBundle) -> Vec<Order> {
        let mut orders = Vec::new();

        for (symbol, qty) in valued_bundle.bundle.0 {
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
                    reduce_only: false,
                    time: valued_bundle
                        .time
                        .expect("Cannot order valued bundle without associated time"),
                    current_price: valued_bundle
                        .valuation
                        .0
                        .get(&symbol)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }

        orders
    }
}

impl Add for &ValuedBundle {
    type Output = ValuedBundle;

    fn add(self, rhs: &ValuedBundle) -> Self::Output {
        assert_eq!(self.valuation, rhs.valuation);
        assert_eq!(self.time, rhs.time);

        let bundle = &self.bundle + &rhs.bundle;

        ValuedBundle {
            bundle,
            valuation: self.valuation.clone(),
            time: self.time,
        }
    }
}

impl Add<&Self> for ValuedBundle {
    type Output = ValuedBundle;

    fn add(self, rhs: &ValuedBundle) -> Self::Output {
        assert_eq!(self.valuation, rhs.valuation);
        assert_eq!(self.time, rhs.time);

        let bundle = self.bundle + &rhs.bundle;

        ValuedBundle {
            bundle,
            valuation: self.valuation,
            time: self.time,
        }
    }
}

impl Neg for &ValuedBundle {
    type Output = ValuedBundle;

    fn neg(self) -> Self::Output {
        ValuedBundle {
            bundle: -&self.bundle,
            valuation: self.valuation.clone(),
            time: self.time,
        }
    }
}

impl Neg for ValuedBundle {
    type Output = ValuedBundle;

    fn neg(self) -> Self::Output {
        ValuedBundle {
            bundle: -self.bundle,
            valuation: self.valuation,
            time: self.time,
        }
    }
}

impl Debug for ValuedBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (symbol, qty) in &self.bundle.0 {
            if *qty != Decimal::ZERO {
                let val = self.valuation.0.get(symbol).cloned().unwrap_or_default();
                write!(f, "{} {} ({} USD), ", qty, symbol, qty * val)?;
            }
        }
        writeln!(f)?;

        Ok(())
    }
}
