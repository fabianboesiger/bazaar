use std::{collections::HashMap, marker::PhantomData};

use crate::{strategies::Settings, AnyError, Api, Exchange, Strategy};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum Trigger {
    StopLoss(Decimal),
    TakeProfit(Decimal),
    TrailingStopLoss(Decimal),
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Close,
    CloseAllAndTimeout(Duration),
    CloseAllAndQuit,
}

struct PositionData {
    max_relative_pnl: Decimal,
    action: Option<Action>,
}

pub struct Levels<A: Api, S: Strategy<A>> {
    _api: PhantomData<A>,
    strategy: S,
    timeout_until: DateTime<Utc>,
    triggers: Vec<(Trigger, Action)>,
    positions: HashMap<Uuid, PositionData>,
}

impl<A: Api, S: Strategy<A>> Levels<A, S> {
    pub fn new(strategy: S) -> Self {
        Levels {
            _api: PhantomData::default(),
            strategy,
            timeout_until: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
            triggers: Vec::new(),
            positions: HashMap::new(),
        }
    }

    pub fn add(mut self, trigger: Trigger, action: Action) -> Self {
        self.triggers.push((trigger, action));
        self
    }
}

impl<A: Api, S: Strategy<A>> Strategy<A> for Levels<A, S> {
    const NAME: &'static str = S::NAME;

    fn init(&mut self, exchange: &mut Exchange<A>) -> Result<Settings, AnyError> {
        self.strategy.init(exchange)
    }

    fn eval(&mut self, exchange: &mut Exchange<A>) -> Result<(), AnyError> {
        self.strategy.eval(exchange)?;

        for position in exchange.positions() {
            let data = self.positions.entry(position.id()).or_insert(PositionData {
                max_relative_pnl: Decimal::ZERO,
                action: None,
            });

            let relative_pnl = position.relative_pnl();
            data.max_relative_pnl = data.max_relative_pnl.max(relative_pnl);

            for &(trigger, action) in &self.triggers {
                if let Some(action) = match trigger {
                    Trigger::StopLoss(threshold) if relative_pnl <= -threshold => Some(action),
                    Trigger::TakeProfit(threshold) if relative_pnl >= threshold => Some(action),
                    Trigger::TrailingStopLoss(threshold)
                        if relative_pnl <= data.max_relative_pnl - threshold =>
                    {
                        Some(action)
                    }
                    _ => None,
                } {
                    log::warn!("Trigger {:?} executing action {:?}", trigger, action);
                    data.action = Some(action);
                }
            }
        }

        let mut quit = false;
        let current_time = exchange.current_time();

        for position in exchange.positions_mut() {
            let data = self.positions.get(&position.id()).unwrap();
            if let Some(action) = data.action {
                match action {
                    Action::Close => {
                        position.close();
                    }
                    Action::CloseAllAndQuit => {
                        quit = true;
                    }
                    Action::CloseAllAndTimeout(duration) => {
                        self.timeout_until = current_time + duration;
                    }
                }
            }
        }

        if quit {
            exchange.close_all();
            exchange.quit();
        }

        if current_time <= self.timeout_until {
            exchange.close_all();
        }

        Ok(())
    }
}
