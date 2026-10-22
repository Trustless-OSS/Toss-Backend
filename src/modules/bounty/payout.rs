use rust_decimal::Decimal;

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayoutDistribution {
    pub address: String,
    pub amount: Decimal,
}

impl PayoutDistribution {
    pub fn validate_total(total: Decimal, distributions: &[Self]) -> Result<(), AppError> {
        if distributions.is_empty()
            || distributions.iter().any(|distribution| {
                distribution.address.trim().is_empty() || distribution.amount <= Decimal::ZERO
            })
        {
            return Err(AppError::bad_request(
                "Payout distributions need a recipient address and a positive amount",
            ));
        }

        let distributed = distributions
            .iter()
            .map(|distribution| distribution.amount)
            .sum::<Decimal>();
        if distributed != total {
            return Err(AppError::bad_request(
                "Payout distributions must add up to the escrowed reward",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayoutRequest {
    Fixed(Decimal),
    Percentage(Decimal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PayoutSplit {
    pub contributor_amount: Decimal,
    pub maintainer_amount: Decimal,
}

impl PayoutRequest {
    pub fn resolve(self, reward: Decimal) -> Result<PayoutSplit, AppError> {
        let contributor_amount = match self {
            Self::Fixed(amount) => amount,
            Self::Percentage(percent) => (reward * percent / Decimal::from(100)).round_dp(7),
        };
        PayoutSplit::from_contributor_amount(reward, contributor_amount)
    }
}

impl PayoutSplit {
    pub fn from_contributor_amount(
        reward: Decimal,
        contributor_amount: Decimal,
    ) -> Result<Self, AppError> {
        if reward <= Decimal::ZERO
            || contributor_amount <= Decimal::ZERO
            || contributor_amount > reward
        {
            return Err(AppError::bad_request(
                "Contributor payout must be greater than 0 and no more than the bounty reward",
            ));
        }
        Ok(Self {
            contributor_amount,
            maintainer_amount: reward - contributor_amount,
        })
    }

    pub fn distributions(
        self,
        contributor_address: &str,
        remainder_address: &str,
    ) -> Result<Vec<PayoutDistribution>, AppError> {
        let mut distributions = Vec::with_capacity(2);
        if contributor_address == remainder_address {
            distributions.push(PayoutDistribution {
                address: contributor_address.to_string(),
                amount: self.contributor_amount + self.maintainer_amount,
            });
        } else {
            distributions.push(PayoutDistribution {
                address: contributor_address.to_string(),
                amount: self.contributor_amount,
            });
            distributions.push(PayoutDistribution {
                address: remainder_address.to_string(),
                amount: self.maintainer_amount,
            });
        }
        PayoutDistribution::validate_total(
            self.contributor_amount + self.maintainer_amount,
            &distributions,
        )?;
        Ok(distributions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_creates_recipient_amount_distributions_that_sum_to_the_escrow() {
        let split =
            PayoutSplit::from_contributor_amount(Decimal::from(10), Decimal::new(4, 0)).unwrap();
        let distributions = split.distributions("contributor", "owner").unwrap();

        assert_eq!(distributions.len(), 2);
        assert_eq!(distributions[0].address, "contributor");
        assert_eq!(distributions[0].amount, Decimal::from(4));
        assert_eq!(distributions[1].address, "owner");
        assert_eq!(distributions[1].amount, Decimal::from(6));
        PayoutDistribution::validate_total(Decimal::from(10), &distributions).unwrap();
    }

    #[test]
    fn generic_distributions_support_multiple_contributors() {
        let distributions = vec![
            PayoutDistribution {
                address: "contributor-one".to_string(),
                amount: Decimal::from(3),
            },
            PayoutDistribution {
                address: "contributor-two".to_string(),
                amount: Decimal::from(2),
            },
            PayoutDistribution {
                address: "owner".to_string(),
                amount: Decimal::from(5),
            },
        ];

        PayoutDistribution::validate_total(Decimal::from(10), &distributions).unwrap();
    }

    #[test]
    fn distributions_must_cover_the_exact_escrow_amount() {
        let distributions = vec![PayoutDistribution {
            address: "contributor".to_string(),
            amount: Decimal::from(4),
        }];

        assert!(PayoutDistribution::validate_total(Decimal::from(10), &distributions).is_err());
    }

    #[test]
    fn equal_recipient_addresses_are_combined() {
        let split =
            PayoutSplit::from_contributor_amount(Decimal::from(10), Decimal::new(4, 0)).unwrap();

        let distributions = split.distributions("same-wallet", "same-wallet").unwrap();

        assert_eq!(distributions.len(), 1);
        assert_eq!(distributions[0].amount, Decimal::from(10));
    }
}
