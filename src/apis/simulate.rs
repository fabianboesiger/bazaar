use super::Api;
use crate::{
    apis::{ApiError, Order, OrderInfo},
    Asset, Candle, CandleKey, Markets, Side, Symbol, Wallet,
};

use async_trait::async_trait;
use futures_util::lock::Mutex;
use rust_decimal::prelude::*;

/// The Simulate API is a middleware that does not actually execute orders,
/// and instead simulates the orders.
/// This is useful for backtesting.
pub struct Simulate<A>
where
    A: Api,
{
    wallet: Mutex<Wallet>,
    api: A,
    //orderbooks: HashMap<Symbol, Orderbook>,
}

impl<A> Simulate<A>
where
    A: Api,
{
    /// Create a simulation middleware for an api by providing a wallet
    /// with your deposit to simulate, and the fee per orders.
    pub fn new(api: A, wallet: Wallet) -> Self {
        Simulate {
            wallet: Mutex::new(wallet),
            api,
            //orderbooks: HashMap::new(),
        }
    }
}

#[async_trait]
impl<A: Api> Api for Simulate<A> {
    const NAME: &'static str = A::NAME;
    const LIVE_TRADING_ENABLED: bool = false;

    async fn get_candles(
        &self,
        key: CandleKey,
    ) -> Result<Vec<(CandleKey, Option<Candle>)>, ApiError> {
        self.api.get_candles(key).await
    }

    async fn place_order(&self, order: Order) -> Result<OrderInfo, ApiError> {
        //let quote_size = order.size * order.price;
        //let wallet = self.wallet.lock().await;

        //wallet.reserve(quote_size, self.quote_asset()).unwrap();
        //wallet.withdraw(quote_size, self.quote_asset()).unwrap();

        Ok(OrderInfo {
            order_id: order.order_id,
            size: order.size,
            price: if order.side == Side::Buy {
                order.current_price * (Decimal::one() + self.api.order_fee().await)
            } else {
                order.current_price * (Decimal::one() - self.api.order_fee().await)
            }
            .round_dp(8),
            time: order.time,
            side: order.side,
            market: order.market,
        })
    }
    /*
    async fn order_update(&self, asset: Asset) -> Pin<Box<dyn Stream<Item = OrderUpdate>>> {
        self.api.order_update(asset).await
    }
    */
    fn format_market(&self, market: Symbol) -> String {
        self.api.format_market(market)
    }

    async fn update_wallet(&self, wallet: &mut Wallet) -> Result<(), ApiError> {
        if wallet.is_fresh() {
            *wallet = self.wallet.lock().await.clone();
        }

        Ok(())
    }

    async fn update_markets(&self, markets: &mut Markets) -> Result<(), ApiError> {
        /*
        markets.markets
            .iter_mut()
            .for_each(|(_symbol, info)| {
                /*
                let candle = cache.1.get(&symbol).unwrap().unwrap();
                let mut bids = BTreeMap::new();
                let mut asks = BTreeMap::new();
                bids.insert(candle.close, Decimal::new(i64::MAX, 0));
                asks.insert(candle.close, Decimal::new(i64::MAX, 0));
                let orderbook = Orderbook { bids, asks };
                */

                *info = MarketInfo {
                    min_size: Decimal::ZERO,
                    size_increment: Decimal::ZERO,
                    price_increment: Decimal::ZERO,
                    daily_volume: Decimal::new(i64::MAX, 0),
                };
            });
        */
        if markets.is_fresh() {
            self.api.update_markets(markets).await?;
        }

        Ok(())
    }

    fn quote_asset(&self) -> Asset {
        self.api.quote_asset()
    }

    async fn order_fee(&self) -> Decimal {
        self.api.order_fee().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{apis::Ftx, OrderType, Side};
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    #[tokio::test]
    async fn deduct_fee_long() {
        let mut wallet = Wallet::new();
        wallet.deposit(dec!(1000), Asset::new("USD"));
        let api = Simulate::new(Ftx::from_env(), wallet);
        let order = Order {
            order_id: Uuid::new_v4(),
            market: Symbol::perp("BTC"),
            side: Side::Buy,
            size: dec!(0.01),
            order_type: OrderType::Market,
            reduce_only: false,
            time: Utc::now(),
            current_price: dec!(10000),
        };

        let OrderInfo { price, .. } = api.place_order(order).await.unwrap();

        assert!(price > dec!(10000));

        let order = Order {
            order_id: Uuid::new_v4(),
            market: Symbol::perp("BTC"),
            side: Side::Sell,
            size: dec!(0.01),
            order_type: OrderType::Market,
            reduce_only: false,
            time: Utc::now(),
            current_price: dec!(10000),
        };

        let OrderInfo { price, .. } = api.place_order(order).await.unwrap();

        assert!(price < dec!(10000));
    }

    #[tokio::test]
    async fn deduct_fee_short() {
        let mut wallet = Wallet::new();
        wallet.deposit(dec!(1000), Asset::new("USD"));
        let api = Simulate::new(Ftx::from_env(), wallet);
        let order = Order {
            order_id: Uuid::new_v4(),
            market: Symbol::perp("BTC"),
            side: Side::Sell,
            size: dec!(0.01),
            order_type: OrderType::Market,
            reduce_only: false,
            time: Utc::now(),
            current_price: dec!(10000),
        };

        let OrderInfo { price, .. } = api.place_order(order).await.unwrap();

        assert!(price < dec!(10000));

        let order = Order {
            order_id: Uuid::new_v4(),
            market: Symbol::perp("BTC"),
            side: Side::Buy,
            size: dec!(0.01),
            order_type: OrderType::Market,
            reduce_only: false,
            time: Utc::now(),
            current_price: dec!(10000),
        };

        let OrderInfo { price, .. } = api.place_order(order).await.unwrap();

        assert!(price > dec!(10000));
    }
}
