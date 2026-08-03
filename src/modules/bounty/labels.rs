use rust_decimal::Decimal;
use serde_json::Value;

use crate::shared::models::{Difficulty, ParsedLabels, Repo};

pub fn parse_labels(labels: &[Value]) -> ParsedLabels {
    let names: Vec<String> = labels
        .iter()
        .filter_map(|label| label.get("name").and_then(|name| name.as_str()))
        .map(|name| name.to_ascii_lowercase())
        .collect();

    let is_rewarded = names.iter().any(|name| name == "rewarded");
    let difficulty = if names.iter().any(|name| name == "manual") {
        Some(Difficulty::Manual)
    } else if names.iter().any(|name| name == "high") {
        Some(Difficulty::High)
    } else if names.iter().any(|name| name == "medium") {
        Some(Difficulty::Medium)
    } else if names.iter().any(|name| name == "low") {
        Some(Difficulty::Low)
    } else {
        None
    };

    ParsedLabels {
        is_rewarded,
        difficulty,
    }
}

pub fn get_reward_amount(
    difficulty: Option<Difficulty>,
    repo: &Repo,
    manual_amount: Option<Decimal>,
) -> Decimal {
    match difficulty {
        Some(Difficulty::Manual) => manual_amount.unwrap_or(Decimal::ZERO),
        Some(Difficulty::High) => repo.reward_high,
        Some(Difficulty::Medium) => repo.reward_medium,
        Some(Difficulty::Low) => repo.reward_low,
        None => Decimal::ZERO,
    }
}

pub fn difficulty_label(difficulty: Difficulty) -> &'static str {
    match difficulty {
        Difficulty::Low => "low",
        Difficulty::Medium => "medium",
        Difficulty::High => "high",
        Difficulty::Manual => "manual",
    }
}
