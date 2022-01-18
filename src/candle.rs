use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;

use crate::Symbol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candle {
    pub close: Decimal,
    pub volume: Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CandleKey {
    pub market: Symbol,
    pub time: DateTime<Utc>,
    pub interval: Duration,
}
