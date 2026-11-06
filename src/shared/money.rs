use rust_decimal::Decimal;

use crate::error::AppError;

pub fn validate_reward(amount: Decimal, allow_zero: bool) -> Result<(), AppError> {
    let max = Decimal::from_i128_with_scale(i64::MAX as i128, 7);
    if amount < Decimal::ZERO
        || (!allow_zero && amount == Decimal::ZERO)
        || amount.normalize().scale() > 7
        || amount > max
    {
        return Err(AppError::bad_request("Invalid reward amount"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn validates_reward_boundaries_and_precision() {
        assert!(validate_reward(Decimal::ZERO, true).is_ok());
        assert!(validate_reward(Decimal::ZERO, false).is_err());
        for amount in ["-1", "0.00000001", "922337203685.4775808"] {
            assert!(validate_reward(Decimal::from_str(amount).unwrap(), true).is_err());
        }
        assert!(validate_reward(Decimal::from_str("922337203685.4775807").unwrap(), false).is_ok());
        assert!(validate_reward(Decimal::from_str("1.00000000").unwrap(), false).is_ok());
    }
}
