/// Canonical labels displayed for milestones in the escrow viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MilestoneStatus {
    Pending,
    Completed,
    Paid,
    Refunded,
    Disputed,
    Resolved,
    Rejected,
}

impl MilestoneStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Completed => "COMPLETED",
            Self::Paid => "PAID",
            Self::Refunded => "REFUNDED",
            Self::Disputed => "DISPUTED",
            Self::Resolved => "RESOLVED",
            Self::Rejected => "REJECTED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MilestoneStatus;

    #[test]
    fn labels_are_uppercase_and_canonical() {
        assert_eq!(MilestoneStatus::Pending.as_str(), "PENDING");
        assert_eq!(MilestoneStatus::Completed.as_str(), "COMPLETED");
        assert_eq!(MilestoneStatus::Paid.as_str(), "PAID");
        assert_eq!(MilestoneStatus::Refunded.as_str(), "REFUNDED");
        assert_eq!(MilestoneStatus::Disputed.as_str(), "DISPUTED");
        assert_eq!(MilestoneStatus::Resolved.as_str(), "RESOLVED");
    }
}
