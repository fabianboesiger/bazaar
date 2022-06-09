use std::hash::BuildHasherDefault;

use crate::Symbol;
use fxhash::{FxHashMap, FxHasher};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Valuation(pub(crate) FxHashMap<Symbol, Decimal>);

impl Default for Valuation {
    fn default() -> Self {
        Self(FxHashMap::with_capacity_and_hasher(
            200,
            BuildHasherDefault::<FxHasher>::default(),
        ))
    }
}
