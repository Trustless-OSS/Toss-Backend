use rust_decimal::Decimal;
use serde_json::Value;

use crate::shared::models::{ParsedLabels, Repo};

/// Parse labels from GitHub issue, matching against repo's configured reward levels.
/// Returns matched reward label (or reserved labels like "manual", "rejected", "rewarded").
pub fn parse_labels(labels: &[Value], reward_labels: &[String]) -> ParsedLabels {
    let names: Vec<String> = labels
        .iter()
        .filter_map(|label| label.get("name").and_then(|name| name.as_str()))
        .map(|name| name.to_ascii_lowercase())
        .collect();

    let is_rewarded = names.iter().any(|name| name == "rewarded");
    let is_rejected = names.iter().any(|name| name == "rejected");
    let is_manual = names.iter().any(|name| name == "manual");

    // Match against repo's configured reward levels first (case-insensitive)
    let matched_level = names.iter().find_map(|name| {
        reward_labels
            .iter()
            .find(|label| label.eq_ignore_ascii_case(name))
            .map(|_| name.clone())
    });

    ParsedLabels {
        is_rewarded,
        is_rejected,
        is_manual,
        matched_level,
    }
}

/// Get reward amount from the matched level label.
pub fn get_reward_amount(
    matched_level: Option<&str>,
    repo: &Repo,
    manual_amount: Option<Decimal>,
) -> Decimal {
    match matched_level {
        Some("manual") => manual_amount.unwrap_or(Decimal::ZERO),
        Some(label) => repo
            .rewards
            .iter()
            .find(|r| r.label.eq_ignore_ascii_case(label))
            .map(|r| r.amount)
            .unwrap_or(Decimal::ZERO),
        None => Decimal::ZERO,
    }
}
