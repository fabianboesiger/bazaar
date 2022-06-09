use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer, Serialize};
use std::{collections::HashSet, fmt, sync::Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Asset(&'static str);

impl<'de> Deserialize<'de> for Asset {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Asset::new)
    }
}

impl Asset {
    // Flyweight pattern
    // Leaks memory if and only if no asset with the same name exists.
    // This allows us to pass the asset name as a static str, which in turn
    // enables implementing Copy.
    pub fn new<R: AsRef<str>>(name: R) -> Self {
        static SET: Lazy<Mutex<HashSet<&'static str>>> = Lazy::new(|| Mutex::new(HashSet::new()));
        let mut set = SET.lock().unwrap();
        if !set.contains(name.as_ref()) {
            let leaked: &'static str = Box::leak(name.as_ref().to_owned().into_boxed_str());
            set.insert(leaked);
        }

        Asset(set.get(name.as_ref()).unwrap())
    }
}

impl fmt::Display for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation() {
        let asset1 = Asset::new("BTC-PERP");
        let asset2 = Asset::new("BTC-PERP");
        let asset3 = Asset::new("ETH-PERP");
        assert!(std::ptr::eq(asset1.0, asset2.0));
        assert!(!std::ptr::eq(asset1.0, asset3.0));
    }
}
