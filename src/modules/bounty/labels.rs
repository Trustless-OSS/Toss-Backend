use rust_decimal::Decimal;
use serde_json::Value;

use crate::shared::models::{ParsedLabels, Repo};

pub fn parse_labels(labels: &[Value], reward_labels: &[String]) -> ParsedLabels {
    let names: Vec<String> = labels
        .iter()
        .filter_map(|label| label.get("name").and_then(|name| name.as_str()))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();

    let is_rewarded = names.iter().any(|name| name == "rewarded");
    let is_rejected = names.iter().any(|name| name == "rejected");
    let is_manual = names.iter().any(|name| name == "manual");

    // Match against repo's configured reward levels first (case-insensitive)
    let matched_level = names.iter().find_map(|name| {
        reward_labels
            .iter()
            .find(|label| label.trim().eq_ignore_ascii_case(name))
            .cloned()
    });

    let matched_level = if is_manual {
        Some("manual".to_string())
    } else {
        matched_level
    };

    ParsedLabels {
        is_rewarded: !is_rejected && is_rewarded,
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
            .find(|r| r.label.trim().eq_ignore_ascii_case(label.trim()))
            .map(|r| r.amount)
            .unwrap_or(Decimal::ZERO),
        None => Decimal::ZERO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn difficulty_alone_does_not_create_bounty_and_preserves_mapping_case() {
        let parsed = parse_labels(&[json!({"name": " SECURITY "})], &["Security".into()]);
        assert!(!parsed.is_rewarded);
        assert_eq!(parsed.matched_level.as_deref(), Some("Security"));
    }

    #[test]
    fn manual_does_not_need_a_configured_reward_mapping() {
        let parsed = parse_labels(
            &[json!({"name": "manual"}), json!({"name": "rewarded"})],
            &[],
        );
        assert!(parsed.is_manual && parsed.is_rewarded);
        assert_eq!(parsed.matched_level.as_deref(), Some("manual"));
    }

    #[test]
    fn rewarded_is_required_with_or_without_a_price_label() {
        for labels in [
            vec![json!({"name": "ReWaRdEd"})],
            vec![json!({"name": "rewarded"}), json!({"name": "low"})],
        ] {
            assert!(parse_labels(&labels, &["low".into()]).is_rewarded);
        }
        assert!(!parse_labels(&[json!({"name": "manual"})], &[]).is_rewarded);
    }

    #[test]
    fn rejected_overrides_reward_labels_and_unrelated_labels_do_not_trigger() {
        assert!(
            !parse_labels(
                &[
                    json!({"name":"rewarded"}),
                    json!({"name":"rejected"}),
                    json!({"name":"gold"})
                ],
                &["gold".into()]
            )
            .is_rewarded
        );
        assert!(!parse_labels(&[json!({"name":"bug"})], &["gold".into()]).is_rewarded);
    }
}
