use super::Valuation;
use crate::Symbol;
use rust_decimal::Decimal;
use std::{
    collections::HashMap,
    ops::{Add, Mul, Neg, Sub},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bundle(pub(crate) HashMap<Symbol, Decimal>);

impl Bundle {
    pub fn invert(&self) -> Self {
        let mut out = self.clone();
        for size in out.0.values_mut() {
            *size = -(*size);
        }
        out
    }

    pub fn abs(&self) -> Self {
        let mut out = self.clone();
        for size in out.0.values_mut() {
            *size = size.abs();
        }
        out
    }

    /*
    pub fn quote_size(self, rhs: &Valuation) -> Decimal {
        let mut value = Decimal::ZERO;

        for (&symbol, price) in &rhs.0 {
            value += self.0.get(&symbol).cloned().unwrap_or_default().abs() * price;
        }

        value
    }

    pub fn signum(&self) -> Self {
        let mut out = self.clone();
        for (_, size) in &mut out.0 {
            *size = size.signum();
        }
        out
    }

    pub(crate) fn buy_only(&self) -> Self {
        let mut bundle = Bundle::default();
        for (&symbol, &size) in &self.0 {
            if size > Decimal::ZERO {
                bundle.0.insert(symbol, size);
            }
        }
        bundle
    }

    pub(crate) fn sell_only(&self) -> Self {
        let mut bundle = Bundle::default();
        for (&symbol, &size) in &self.0 {
            if size < Decimal::ZERO {
                bundle.0.insert(symbol, size);
            }
        }
        bundle
    }
    */
}

impl Add for &Bundle {
    type Output = Bundle;

    fn add(self, rhs: &Bundle) -> Self::Output {
        let mut output = HashMap::new();

        for (&symbol, qty) in &self.0 {
            *output.entry(symbol).or_default() += qty;
        }

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() += qty;
        }

        Bundle(output)
    }
}

impl Sub for &Bundle {
    type Output = Bundle;

    fn sub(self, rhs: &Bundle) -> Self::Output {
        let mut output = HashMap::new();

        for (&symbol, qty) in &self.0 {
            *output.entry(symbol).or_default() += qty;
        }

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() -= qty;
        }

        Bundle(output)
    }
}

impl Mul<&Valuation> for &Bundle {
    type Output = Decimal;

    fn mul(self, rhs: &Valuation) -> Self::Output {
        let mut value = Decimal::ZERO;

        for (&symbol, price) in &rhs.0 {
            value += self.0.get(&symbol).cloned().unwrap_or_default() * price;
        }

        value
    }
}

impl Mul<&Bundle> for &Bundle {
    type Output = Bundle;

    fn mul(self, rhs: &Bundle) -> Self::Output {
        let mut output = HashMap::new();

        for (&symbol, &qty) in &self.0 {
            output.insert(
                symbol,
                qty * rhs.0.get(&symbol).cloned().unwrap_or_default(),
            );
        }

        Bundle(output)
    }
}

impl Neg for &Bundle {
    type Output = Bundle;

    fn neg(self) -> Self::Output {
        let mut output = HashMap::new();

        for (&symbol, &qty) in &self.0 {
            output.insert(symbol, -qty);
        }

        Bundle(output)
    }
}
