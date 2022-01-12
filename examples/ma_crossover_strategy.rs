use bazaar::{
    apis::{Api, Ftx, Simulate, Store},
    strategies::{Monitor, Options, Strategy},
    AnyError, Asset, Exchange, Position, Side, Symbol, Wallet, PositionData,
};
use chrono::{Duration, TimeZone, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use rolling_norm::Series;

pub struct MaCrossoverStrategy<const FAST: usize, const SLOW: usize> {
    // Keep track of a two series to compute the moving averages.
    fast: Series<f32, FAST>,
    slow: Series<f32, SLOW>,
    // The symbol to trade on.
    symbol: Symbol,
}

impl<const FAST: usize, const SLOW: usize> MaCrossoverStrategy<FAST, SLOW> {
    fn new() -> Self {
        MaCrossoverStrategy {
            fast: Series::new(),
            slow: Series::new(),
            symbol: Symbol::perp("BTC"),
        }
    }
}

// This strategy is applicable for all APIs that allow futures trading.
impl<A: Api, const FAST: usize, const SLOW: usize> Strategy<A> for MaCrossoverStrategy<FAST, SLOW> {
    const NAME: &'static str = "MA Crossover Strategy";

    fn init(&mut self, exchange: &mut Exchange<A>) -> Result<Options, AnyError> {
        // Begin watching the BTC-PERP ticker as we want to trade it.
        exchange.watch(self.symbol);
        Ok(Options {
            // We trade on the one minute interval.
            interval: Duration::minutes(1),
            ..Default::default()
        })
    }

    fn eval(&mut self, exchange: &mut Exchange<A>) -> Result<(), AnyError> {
        let price = exchange.candle(self.symbol).unwrap().close.to_f32().unwrap();
        self.fast.insert(price);
        self.slow.insert(price);

        let position = exchange
            .prepare(Position {
                symbol: self.symbol,
                size: dec!(0.01),
                side: Side::Buy,
            })?;
        exchange.enter(position);
        
        for position in exchange.open_positions() {
            exchange.exit(position);
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .with_utc_timestamps()
        .init()
        .unwrap();

    // Set up wallet for simulation.
    let mut wallet = Wallet::new();
    wallet.deposit(dec!(1000), Asset::new("USD"));

    // Set up API. FTX as backend with local store enabled and simulated trades.
    let api = Simulate::new(Store::new(Ftx::new()).await, wallet, dec!(0.001));

    // Set up exchange using the API as defined above.
    let exchange = Exchange::new(api, Utc.ymd(2021, 6, 1).and_hms(0, 0, 0));

    // Create strategy instance, and monitor the performance.
    let strategy = Monitor::new(MaCrossoverStrategy::<20, 40>::new(), "127.0.0.1:4444".parse()?);

    // Run the strategy on the exchange.
    exchange.run(strategy).await
}
