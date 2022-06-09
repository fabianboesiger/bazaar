use super::Valuation;
use crate::Symbol;
use fxhash::{FxHashMap, FxHasher};
use rust_decimal::{prelude::Signed, Decimal};
use std::{
    hash::BuildHasherDefault,
    ops::{Add, Mul, Neg, Sub},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle(pub(crate) FxHashMap<Symbol, Decimal>);

impl Default for Bundle {
    fn default() -> Self {
        Self(FxHashMap::with_capacity_and_hasher(
            200,
            BuildHasherDefault::<FxHasher>::default(),
        ))
    }
}

impl Bundle {
    pub fn abs(&self) -> Self {
        let mut out = self.clone();
        for size in out.0.values_mut() {
            *size = size.abs();
        }
        out
    }

    pub fn signum(&self) -> Self {
        let mut out = self.clone();
        for size in out.0.values_mut() {
            *size = size.signum();
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
        let mut output = self.0.clone();

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() += qty;
        }

        Bundle(output)
    }
}

impl Add<&Self> for Bundle {
    type Output = Bundle;

    fn add(self, rhs: &Bundle) -> Self::Output {
        let mut output = self.0;

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() += qty;
        }

        Bundle(output)
    }
}

impl Sub for &Bundle {
    type Output = Bundle;

    fn sub(self, rhs: &Bundle) -> Self::Output {
        let mut output = self.0.clone();

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() -= qty;
        }

        Bundle(output)
    }
}

impl Sub<&Self> for Bundle {
    type Output = Bundle;

    fn sub(self, rhs: &Bundle) -> Self::Output {
        let mut output = self.0;

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() -= qty;
        }

        Bundle(output)
    }
}

impl Mul for &Bundle {
    type Output = Bundle;

    fn mul(self, rhs: &Bundle) -> Self::Output {
        let mut output = self.0.clone();

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() *= qty;
        }

        Bundle(output)
    }
}

impl Mul<&Self> for Bundle {
    type Output = Bundle;

    fn mul(self, rhs: &Bundle) -> Self::Output {
        let mut output = self.0;

        for (&symbol, qty) in &rhs.0 {
            *output.entry(symbol).or_default() *= qty;
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

impl Neg for &Bundle {
    type Output = Bundle;

    fn neg(self) -> Self::Output {
        let mut output = self.0.clone();

        for value in output.values_mut() {
            *value = -*value;
        }

        Bundle(output)
    }
}

impl Neg for Bundle {
    type Output = Bundle;

    fn neg(self) -> Self::Output {
        let mut output = self.0;

        for value in output.values_mut() {
            *value = -*value;
        }

        Bundle(output)
    }
}
