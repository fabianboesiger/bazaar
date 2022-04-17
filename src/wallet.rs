use rust_decimal::Decimal;
use std::collections::HashMap;
use thiserror::Error;

use crate::Asset;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Not enough total available.")]
    NotEnoughTotal,
    #[error("Not enough reserved.")]
    NotEnoughReserved,
}

#[derive(Default, Debug, Clone)]
pub struct Wallet {
    pub(crate) total: HashMap<Asset, Decimal>,
    pub(crate) free: HashMap<Asset, Decimal>,
}

impl Wallet {
    pub fn new() -> Self {
        Default::default()
    }

    pub(crate) fn is_fresh(&self) -> bool {
        self.total.is_empty()
    }

    pub fn assets(&self) -> impl Iterator<Item = (&Asset, &Decimal)> {
        self.total.iter()
    }

    pub fn deposit(&mut self, qty: Decimal, asset: Asset) {
        assert!(qty >= Decimal::ZERO);
        log::debug!("Depositing {} {}", qty, asset);
        let mut total_qty = self.total.entry(asset).or_default();
        let mut free_qty = self.free.entry(asset).or_default();
        total_qty += qty;
        free_qty += qty;
    }

    pub fn reserve(&mut self, qty: Decimal, asset: Asset) -> Result<(), WalletError> {
        assert!(qty >= Decimal::ZERO);
        let mut free_qty = self.free.entry(asset).or_default();
        log::debug!("Reserving {} {}", qty, asset);
        if qty > *free_qty {
            return Err(WalletError::NotEnoughTotal);
        }
        free_qty -= qty;
        Ok(())
    }

    pub fn unreserve(&mut self, qty: Decimal, asset: Asset) -> Result<(), WalletError> {
        assert!(qty >= Decimal::ZERO);
        let mut free_qty = self.free.entry(asset).or_default();
        let total_qty = self.total.entry(asset).or_default();
        let reserved_qty = *total_qty - *free_qty;
        if qty > reserved_qty {
            return Err(WalletError::NotEnoughReserved);
        }
        free_qty += qty;
        Ok(())
    }

    pub fn unreserve_all(&mut self, asset: Asset) {
        let mut free_qty = self.free.entry(asset).or_default();
        let total_qty = self.total.entry(asset).or_default();
        let reserved_qty = *total_qty - *free_qty;
        free_qty += reserved_qty;
    }

    pub fn free(&self, asset: Asset) -> Decimal {
        self.free.get(&asset).cloned().unwrap_or(Decimal::ZERO)
    }

    pub fn total(&self, asset: Asset) -> Decimal {
        self.total.get(&asset).cloned().unwrap_or(Decimal::ZERO)
    }

    /// Withdraw some quantity of an asset.
    /// Assumes that the quantity to be withdrawn was reserved beforehand.
    pub fn withdraw(&mut self, qty: Decimal, asset: Asset) -> Result<(), WalletError> {
        assert!(qty >= Decimal::ZERO);
        log::debug!("Withdrawing {} {}", qty, asset);
        let mut total_qty = self.total.entry(asset).or_default();
        let free_qty = self.free.entry(asset).or_default();
        let reserved_qty = *total_qty - *free_qty;
        if qty > reserved_qty {
            return Err(WalletError::NotEnoughReserved);
        }
        total_qty -= qty;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn deposit_withdraw() {
        let mut wallet = Wallet::new();
        let asset = Asset::new("BTC");
        wallet.deposit(dec!(10), asset);
        wallet.withdraw(dec!(10), asset).unwrap_err();
        wallet.reserve(dec!(10), asset).unwrap();
        wallet.withdraw(dec!(10), asset).unwrap();
    }
}
