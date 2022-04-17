use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Symbol;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

/// Defines an order that can be placed in an exchange.
#[derive(Debug, Clone)]
pub struct Order {
    pub order_id: Uuid,
    pub market: Symbol,
    pub side: Side,
    pub size: Decimal,
    pub order_type: OrderType,
    pub reduce_only: bool,
    pub time: DateTime<Utc>,
    pub current_price: Decimal,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum OrderType {
    Limit(Decimal),
    Market,
}

#[derive(Debug, Clone)]
pub struct OrderInfo {
    pub order_id: Uuid,
    pub market: Symbol,
    pub size: Decimal,
    pub price: Decimal,
    pub time: DateTime<Utc>,
    pub side: Side,
}
