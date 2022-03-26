use chrono::Duration;

use crate::{apis::Api, AnyError, Exchange};

/// This trait needs to be implemented by your strategy.
pub trait Strategy<A>
where
    A: Api,
{
    const NAME: &'static str;
    /// This method is called once at the start of the strategy.
    fn init(&mut self, manager: &mut Exchange<A>) -> Result<Settings, AnyError>;
    /// This method is called after each interval.
    fn eval(&mut self, manager: &mut Exchange<A>) -> Result<(), AnyError>;
}

pub struct Settings {
    /// Specifies the interval on which to trade on.
    pub interval: Duration,
    /// Specifies how errors caused by the strategy should be handled,
    pub on_error: OnError,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interval: Duration::minutes(1),
            on_error: OnError::ExitAllPositionsAndReturn,
        }
    }
}

#[derive(Clone, Copy)]
pub enum OnError {
    /// Stop running the strategy and return the error.
    Return,
    /// If an error occurs, exit all positions and return the error.
    ExitAllPositionsAndReturn,
    /// If an error occurs, exit all positions and return the error.
    ExitAllPositionsAndResume,
}
