use bazaar::{
    apis::{Api, Ftx},
    strategies::{Settings, Strategy},
    AnyError, Bazaar, Exchange, Position, Symbol,
};
use chrono::{Duration, TimeZone, Utc};
use rolling_norm::Series;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;

// Implements a simple MA crossover strategy using two moving averages with periods FAST and SLOW.
pub struct MaCrossoverStrategy<const FAST: usize, const SLOW: usize> {
    // Keep track of a two series to compute the moving averages.
    fast: Series<f32, FAST>,
    slow: Series<f32, SLOW>,
    // The symbol to trade on.
    symbol: Symbol,
    last_long_crossover: bool,
}

impl<const FAST: usize, const SLOW: usize> MaCrossoverStrategy<FAST, SLOW> {
    pub fn new() -> Self {
        MaCrossoverStrategy {
            fast: Series::new(),
            slow: Series::new(),
            symbol: Symbol::perp("BTC"),
            last_long_crossover: false,
        }
    }
}

// This strategy is applicable for all APIs that allow futures trading.
impl<A: Api, const FAST: usize, const SLOW: usize> Strategy<A> for MaCrossoverStrategy<FAST, SLOW> {
    const NAME: &'static str = "MA Crossover Strategy";

    // Inititalize the strategy.
    fn init(&mut self, exchange: &mut Exchange<A>) -> Result<Settings, AnyError> {
        // Begin watching the BTC-PERP ticker as we want to trade it.
        exchange.watch(self.symbol);

        Ok(Settings {
            // We trade on the one minute interval.
            interval: Duration::minutes(1),
            ..Default::default()
        })
    }

    // Evaluate the strategy periodically.
    fn eval(&mut self, exchange: &mut Exchange<A>) -> Result<(), AnyError> {
        let price = exchange
            .candle(self.symbol)
            .unwrap()
            .close
            .to_f32()
            .unwrap();

        self.fast.insert(price);
        self.slow.insert(price);

        let curr_long_crossover = self.fast.mean() > self.slow.mean();

        if curr_long_crossover && !self.last_long_crossover {
            // exit all positions and go long.
            exchange.close_all();
            let mut position = Position::new();
            *position.size(self.symbol) = dec!(0.01);
            exchange.open(position)?;
        } else if !curr_long_crossover && self.last_long_crossover {
            // exit all positions and go short.
            exchange.close_all();
            let mut position = Position::new();
            // We go short by setting a negative size.
            *position.size(self.symbol) = dec!(-0.01);
            exchange.open(position)?;
        }

        self.last_long_crossover = curr_long_crossover;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Trace)
        .with_utc_timestamps()
        .init()
        .unwrap();

    Bazaar {
        start_time: Utc.ymd(2022, 1, 10).and_hms(0, 0, 0),
        ..Default::default()
    }
    .run(Ftx::new(), MaCrossoverStrategy::<20, 40>::new())
    .await
}
