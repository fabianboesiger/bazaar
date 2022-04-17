use crate::Asset;
use rust_decimal::prelude::*;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt};

pub struct Markets {
    pub(crate) markets: HashMap<Symbol, MarketInfo>,
}

impl Markets {
    pub fn new() -> Self {
        Markets {
            markets: HashMap::new(),
        }
    }

    pub(crate) fn is_fresh(&self) -> bool {
        self.markets.is_empty()
    }

    pub fn market(&self, symbol: Symbol) -> Option<&MarketInfo> {
        self.markets.get(&symbol)
    }

    pub fn markets(&self) -> impl Iterator<Item = (&Symbol, &MarketInfo)> {
        self.markets.iter()
    }
}

/*
pub struct Markets {
    pub(crate) markets: HashMap<Symbol, Market>,
}

impl Markets {
    pub fn new() -> Self {
        Markets {
            markets: HashMap::new(),
        }
    }

    pub fn market(&self, symbol: Symbol) -> Option<&Market> {
        self.markets.get(&symbol)
    }

    pub fn markets(&self) -> impl Iterator<Item = (&Symbol, &Market)> {
        self.markets.iter()
    }
}

pub struct Market {
    pub(crate) info: MarketInfo,
    pub(crate) orderbook: Orderbook,
}

impl Market {
    pub fn orderbook(&self) -> &Orderbook {
        &self.orderbook
    }

    pub fn info(&self) -> &MarketInfo {
        &self.info
    }
}
*/

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Symbol {
    //Spot(Asset, Asset),
    Perp(Asset),
}

impl Symbol {
    pub(crate) fn new<T: AsRef<str>>(string: T) -> Self {
        match string.as_ref().split_once("-") {
            None => unreachable!(),
            /*match string.as_ref().split_once("/") {
                None => unreachable!(),
                Some((base, quote)) => Symbol::Spot(Asset::new(base), Asset::new(quote)),
            },*/
            Some((underlying, "PERP")) => Symbol::Perp(Asset::new(underlying)),
            _ => unreachable!(),
        }
    }
    /*
    pub fn spot<T: AsRef<str>>(base: T, quote: T) -> Self {
        Symbol::Spot(Asset::new(base), Asset::new(quote))
    }
    */
    pub fn perp<T: AsRef<str>>(underlying: T) -> Self {
        Symbol::Perp(Asset::new(underlying))
    }
    /*
    pub fn base_asset(&self) -> Asset {
        match self {
            Self::Spot(base, _) => *base,
            Self::Perp(base) => *base,
        }
    }

    pub fn quote_asset(&self) -> Asset {
        match self {
            Self::Spot(_, quote) => *quote,
            Self::Perp(_) => Asset::new("USD"),
        }
    }
    */
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            //Self::Spot(base, quote) => write!(f, "{}/{}", base, quote),
            Self::Perp(asset) => write!(f, "{}-PERP", asset),
        }
    }
}

/*
#[derive(Debug, Clone)]
pub struct Orderbook {
    pub bids: BTreeMap<Decimal, Decimal>,
    pub asks: BTreeMap<Decimal, Decimal>,
}
impl Orderbook {
    pub fn new() -> Orderbook {
        Orderbook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    /// Returns the price of the best bid
    pub fn bid_price(&self) -> Option<Decimal> {
        self.bids.keys().rev().next().cloned()
    }

    /// Returns the price of the best ask
    pub fn ask_price(&self) -> Option<Decimal> {
        self.asks.keys().next().cloned()
    }

    /// Returns the midpoint between the best bid price and best ask price.
    /// Output is not rounded to the smallest price increment.
    pub fn mid_price(&self) -> Option<Decimal> {
        Some((self.bid_price()? + self.ask_price()?) / Decimal::new(2, 0))
    }

    /// Returns the expected execution price of a market order given the current
    /// orders in the order book. Returns None if the order size exceeds the
    /// liquidity available on that side of the order book.
    pub fn execution_price(&self, size: Decimal, side: Side) -> Option<Decimal> {
        // Match with orders in the book
        let mut bids_iter = self.bids.iter().rev();
        let mut asks_iter = self.asks.iter();

        let mut fills: Vec<(Decimal, Decimal)> = Vec::new(); // (price, quantity)
        let mut remaining = size;

        while remaining > Decimal::ZERO {
            let (price, quantity) = match side {
                Side::Buy => asks_iter.next()?,
                Side::Sell => bids_iter.next()?,
            };

            if *quantity <= remaining {
                remaining -= quantity;
                fills.push((*price, *quantity));
            } else {
                fills.push((*price, remaining));
                remaining = Decimal::ZERO;
            }
        }

        // Compute the weighted average
        let mut weighted_avg = Decimal::ZERO;
        for (fill_price, fill_quantity) in fills.iter() {
            weighted_avg += fill_price * fill_quantity;
        }

        Some(weighted_avg / size)
    }
}
*/

/*
pub struct Market {
    market_info: MarketInfo,
    // The current candle.
    //candles: VecDeque<(CandleKey, Candle)>,
}
*/

#[derive(Clone, Copy, Debug)]
pub struct MarketInfo {
    pub symbol: Symbol,
    pub min_size: Decimal,
    pub size_increment: Decimal,
    pub price_increment: Decimal,
    pub daily_quote_volume: Decimal,
}
