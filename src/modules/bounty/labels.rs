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
    let find_reward = |label: &str| {
        repo.rewards
            .iter()
            .find(|r| r.label.eq_ignore_ascii_case(label))
            .map(|r| r.amount)
            .unwrap_or(Decimal::ZERO)
    };

    match difficulty {
        Some(Difficulty::Manual) => manual_amount.unwrap_or(Decimal::ZERO),
        Some(Difficulty::High) => find_reward("high"),
        Some(Difficulty::Medium) => find_reward("medium"),
        Some(Difficulty::Low) => find_reward("low"),
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
