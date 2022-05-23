use crate::Symbol;
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Valuation(pub(crate) HashMap<Symbol, Decimal>);
