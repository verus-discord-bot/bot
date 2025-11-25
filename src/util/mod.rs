use std::{
    fmt::Display,
    ops::{Add, Div, Mul, Sub},
};

use num_traits::{CheckedDiv, CheckedMul, CheckedSub, FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;

use serde::Deserialize;
use vrsc::Amount as VrscAmount;

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Amount(Decimal);

impl Amount {
    pub const ZERO: Amount = Amount(Decimal::ZERO);
    pub const MAX: Amount = Amount(Decimal::MAX);
    pub const MIN: Amount = Amount(Decimal::MIN);

    pub fn new(decimal: Decimal) -> Self {
        Self(decimal)
    }

    pub fn inner(&self) -> Decimal {
        self.0
    }

    pub fn from_sat(value: u64) -> Self {
        Self::from(value as f64 / 1_0000_0000.0)
    }
}

impl Display for Amount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

impl Sub for Amount {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl CheckedSub for Amount {
    fn checked_sub(&self, v: &Self) -> Option<Self> {
        self.0.checked_sub(v.0).map(Self)
    }
}

impl Mul for Amount {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Amount(self.0.mul(rhs.0))
    }
}

impl CheckedMul for Amount {
    fn checked_mul(&self, v: &Self) -> Option<Self> {
        self.0.checked_mul(v.0).map(Self)
    }
}

impl Add for Amount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Amount(self.0.add(rhs.0))
    }
}

impl Div for Amount {
    type Output = Self;

    fn div(self, rhs: Self) -> Self::Output {
        Self(self.0.div(rhs.0))
    }
}

impl CheckedDiv for Amount {
    fn checked_div(&self, v: &Self) -> Option<Self> {
        self.0.checked_div(v.0).map(Self)
    }
}

impl From<VrscAmount> for Amount {
    fn from(value: VrscAmount) -> Self {
        Self::from(value.as_vrsc())
    }
}

impl TryInto<VrscAmount> for Amount {
    type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

    fn try_into(self) -> Result<VrscAmount, Self::Error> {
        Ok(VrscAmount::from_vrsc(
            self.0
                .round_dp_with_strategy(8, rust_decimal::RoundingStrategy::ToZero)
                .to_f64()
                .ok_or("Could not round")?,
        )?)
    }
}

impl From<u64> for Amount {
    fn from(value: u64) -> Self {
        Amount(Decimal::from_u64(value).expect("A u64 always fits in a Decimal"))
    }
}

impl From<f64> for Amount {
    fn from(value: f64) -> Self {
        Amount(Decimal::from_f64(value).expect("A f64 always fits in a Decimal"))
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::dec;

    use super::*;

    #[test]
    fn test_vrsc_to_decimal() {
        let vrsc_amount = VrscAmount::from_sat(1_0000_0000);
        let amount = Amount::from(vrsc_amount);

        assert_eq!(amount, Amount(dec!(1.0)));

        let vrsc_amount = VrscAmount::from_sat(80_000_000_0000_0000);
        let amount = Amount::from(vrsc_amount);
        assert_eq!(amount, Amount(dec!(80_000_000.0)));
    }
}
