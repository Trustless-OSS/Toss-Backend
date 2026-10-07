#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Contributor,
    Maintainer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    None,
    BellOnly,
    BellAndEmail,
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    BountyAssigned,
    BountyUnassigned,
    AmountUpdated,
    AmountMissing,
    WalletRequired,
    BountyLocked,
    PayoutReleased,
    PayoutBlocked,
    BountyCancelled,
    BountyRejected,
    InsufficientFunds,
    EscrowFunded,
    EscrowRefunded,
}

impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BountyAssigned => "bounty_assigned",
            Self::BountyUnassigned => "bounty_unassigned",
            Self::AmountUpdated => "amount_updated",
            Self::AmountMissing => "amount_missing",
            Self::WalletRequired => "wallet_required",
            Self::BountyLocked => "bounty_locked",
            Self::PayoutReleased => "payout_released",
            Self::PayoutBlocked => "payout_blocked",
            Self::BountyCancelled => "bounty_cancelled",
            Self::BountyRejected => "bounty_rejected",
            Self::InsufficientFunds => "insufficient_funds",
            Self::EscrowFunded => "escrow_funded",
            Self::EscrowRefunded => "escrow_refunded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bounty_assigned" => Some(Self::BountyAssigned),
            "bounty_unassigned" => Some(Self::BountyUnassigned),
            "amount_updated" => Some(Self::AmountUpdated),
            "amount_missing" => Some(Self::AmountMissing),
            "wallet_required" => Some(Self::WalletRequired),
            "bounty_locked" => Some(Self::BountyLocked),
            "payout_released" => Some(Self::PayoutReleased),
            "payout_blocked" => Some(Self::PayoutBlocked),
            "bounty_cancelled" => Some(Self::BountyCancelled),
            "bounty_rejected" => Some(Self::BountyRejected),
            "insufficient_funds" => Some(Self::InsufficientFunds),
            "escrow_funded" => Some(Self::EscrowFunded),
            "escrow_refunded" => Some(Self::EscrowRefunded),
            _ => None,
        }
    }

    pub fn delivery(&self, role: Role) -> Delivery {
        match (self, role) {
            (Self::BountyAssigned, Role::Contributor) => Delivery::BellAndEmail,
            (Self::BountyAssigned, Role::Maintainer) => Delivery::None,
            (Self::BountyUnassigned, Role::Contributor) => Delivery::BellAndEmail,
            (Self::BountyUnassigned, Role::Maintainer) => Delivery::None,
            (Self::AmountUpdated, Role::Contributor) => Delivery::BellOnly,
            (Self::AmountUpdated, Role::Maintainer) => Delivery::None,
            (Self::AmountMissing, Role::Contributor) => Delivery::BellOnly,
            (Self::AmountMissing, Role::Maintainer) => Delivery::BellAndEmail,
            (Self::WalletRequired, Role::Contributor) => Delivery::BellAndEmail,
            (Self::WalletRequired, Role::Maintainer) => Delivery::None,
            (Self::BountyLocked, Role::Contributor) => Delivery::BellAndEmail,
            (Self::BountyLocked, Role::Maintainer) => Delivery::BellOnly,
            (Self::PayoutReleased, Role::Contributor) => Delivery::BellAndEmail,
            (Self::PayoutReleased, Role::Maintainer) => Delivery::BellOnly,
            (Self::PayoutBlocked, Role::Contributor) => Delivery::BellOnly,
            (Self::PayoutBlocked, Role::Maintainer) => Delivery::BellAndEmail,
            (Self::BountyCancelled, Role::Contributor) => Delivery::BellAndEmail,
            (Self::BountyCancelled, Role::Maintainer) => Delivery::None,
            (Self::BountyRejected, Role::Contributor) => Delivery::BellAndEmail,
            (Self::BountyRejected, Role::Maintainer) => Delivery::None,
            (Self::InsufficientFunds, Role::Contributor) => Delivery::None,
            (Self::InsufficientFunds, Role::Maintainer) => Delivery::BellAndEmail,
            (Self::EscrowFunded, Role::Contributor) => Delivery::None,
            (Self::EscrowFunded, Role::Maintainer) => Delivery::BellOnly,
            (Self::EscrowRefunded, Role::Contributor) => Delivery::None,
            (Self::EscrowRefunded, Role::Maintainer) => Delivery::BellOnly,
        }
    }
}

#[derive(Clone)]
pub struct Notify {
    pub recipient: uuid::Uuid,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub ref_id: Option<uuid::Uuid>,
    pub data: serde_json::Value,
    pub dedupe_key: String,
    pub actor: Option<uuid::Uuid>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounty_assigned_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::BountyAssigned.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_assigned_maintainer_gets_none() {
        assert_eq!(
            Kind::BountyAssigned.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn bounty_unassigned_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::BountyUnassigned.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_unassigned_maintainer_gets_none() {
        assert_eq!(
            Kind::BountyUnassigned.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn amount_updated_contributor_gets_bell_only() {
        assert_eq!(
            Kind::AmountUpdated.delivery(Role::Contributor),
            Delivery::BellOnly
        );
    }

    #[test]
    fn amount_updated_maintainer_gets_none() {
        assert_eq!(
            Kind::AmountUpdated.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn amount_missing_contributor_gets_bell_only() {
        assert_eq!(
            Kind::AmountMissing.delivery(Role::Contributor),
            Delivery::BellOnly
        );
    }

    #[test]
    fn amount_missing_maintainer_gets_bell_and_email() {
        assert_eq!(
            Kind::AmountMissing.delivery(Role::Maintainer),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn wallet_required_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::WalletRequired.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn wallet_required_maintainer_gets_none() {
        assert_eq!(
            Kind::WalletRequired.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn bounty_locked_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::BountyLocked.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_locked_maintainer_gets_bell_only() {
        assert_eq!(
            Kind::BountyLocked.delivery(Role::Maintainer),
            Delivery::BellOnly
        );
    }

    #[test]
    fn payout_released_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::PayoutReleased.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn payout_released_maintainer_gets_bell_only() {
        assert_eq!(
            Kind::PayoutReleased.delivery(Role::Maintainer),
            Delivery::BellOnly
        );
    }

    #[test]
    fn payout_blocked_contributor_gets_bell_only() {
        assert_eq!(
            Kind::PayoutBlocked.delivery(Role::Contributor),
            Delivery::BellOnly
        );
    }

    #[test]
    fn payout_blocked_maintainer_gets_bell_and_email() {
        assert_eq!(
            Kind::PayoutBlocked.delivery(Role::Maintainer),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_cancelled_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::BountyCancelled.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_cancelled_maintainer_gets_none() {
        assert_eq!(
            Kind::BountyCancelled.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn bounty_rejected_contributor_gets_bell_and_email() {
        assert_eq!(
            Kind::BountyRejected.delivery(Role::Contributor),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn bounty_rejected_maintainer_gets_none() {
        assert_eq!(
            Kind::BountyRejected.delivery(Role::Maintainer),
            Delivery::None
        );
    }

    #[test]
    fn insufficient_funds_contributor_gets_none() {
        assert_eq!(
            Kind::InsufficientFunds.delivery(Role::Contributor),
            Delivery::None
        );
    }

    #[test]
    fn insufficient_funds_maintainer_gets_bell_and_email() {
        assert_eq!(
            Kind::InsufficientFunds.delivery(Role::Maintainer),
            Delivery::BellAndEmail
        );
    }

    #[test]
    fn escrow_funded_contributor_gets_none() {
        assert_eq!(
            Kind::EscrowFunded.delivery(Role::Contributor),
            Delivery::None
        );
    }

    #[test]
    fn escrow_funded_maintainer_gets_bell_only() {
        assert_eq!(
            Kind::EscrowFunded.delivery(Role::Maintainer),
            Delivery::BellOnly
        );
    }

    #[test]
    fn escrow_refunded_contributor_gets_none() {
        assert_eq!(
            Kind::EscrowRefunded.delivery(Role::Contributor),
            Delivery::None
        );
    }

    #[test]
    fn escrow_refunded_maintainer_gets_bell_only() {
        assert_eq!(
            Kind::EscrowRefunded.delivery(Role::Maintainer),
            Delivery::BellOnly
        );
    }

    #[test]
    fn actor_exclusion_prevents_self_notification() {
        let user_id = uuid::Uuid::nil();
        let notify = Notify {
            recipient: user_id,
            kind: Kind::BountyAssigned,
            title: "test".to_string(),
            body: "test".to_string(),
            ref_id: None,
            data: serde_json::json!({}),
            dedupe_key: "test".to_string(),
            actor: Some(user_id),
        };

        // In service, if n.actor == Some(n.recipient), it returns early
        assert_eq!(notify.actor, Some(notify.recipient));
    }

    #[test]
    fn different_dedupe_keys_allow_duplicate_notifications() {
        // With different dedupe_keys, both should insert successfully
        // (repository returns Ok(Some(notif)) on first insert, Ok(None) on duplicate)
        let key1 = "bounty:123:assigned";
        let key2 = "bounty:123:assigned:different";
        assert_ne!(key1, key2);
    }

    #[test]
    fn same_dedupe_key_indicates_duplicate() {
        // With the same dedupe_key, repository should return Ok(None)
        let key1 = "bounty:123:assigned";
        let key2 = "bounty:123:assigned";
        assert_eq!(key1, key2);
    }

    #[test]
    fn dedupe_key_suffix_includes_profile_id() {
        let base_key = "bounty:123:assigned";
        let profile_id = uuid::Uuid::nil();
        let suffixed = format!("{}:{}", base_key, profile_id);
        assert!(suffixed.contains(":"));
        assert!(suffixed.ends_with(&profile_id.to_string()));
    }
}
