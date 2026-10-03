#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Assigned,
    Unassigned,
    AmountUpdated,
    AmountMissing,
    WalletRequired,
    Locked,
    Released,
    Blocked,
    Cancelled,
    Rejected,
    InsufficientFunds,
    Funded,
    Refunded,
}
impl Kind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Assigned => "bounty_assigned",
            Self::Unassigned => "bounty_unassigned",
            Self::AmountUpdated => "amount_updated",
            Self::AmountMissing => "amount_missing",
            Self::WalletRequired => "wallet_required",
            Self::Locked => "bounty_locked",
            Self::Released => "payout_released",
            Self::Blocked => "payout_blocked",
            Self::Cancelled => "bounty_cancelled",
            Self::Rejected => "bounty_rejected",
            Self::InsufficientFunds => "insufficient_funds",
            Self::Funded => "escrow_funded",
            Self::Refunded => "escrow_refunded",
        }
    }
}

pub struct Notify {
    pub recipient: uuid::Uuid,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub ref_id: Option<uuid::Uuid>,
    pub data: serde_json::Value,
    pub dedupe_key: String,
    pub email: bool,
}
