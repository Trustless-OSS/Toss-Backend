use rust_decimal::Decimal;
use serde_json::Value;

use crate::shared::models::{ParsedLabels, Repo};

pub fn parse_labels(labels: &[Value], reward_labels: &[String]) -> ParsedLabels {
    let names: Vec<String> = labels
        .iter()
        .filter_map(|label| label.get("name").and_then(|name| name.as_str()))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();

    let is_rewarded = names
        .iter()
        .any(|name| name == "rewarded" || name == "trustless-oss" || name == "bounty");
    let is_rejected = names
        .iter()
        .any(|name| name == "rejected" || name == "reject");
    let is_manual = names
        .iter()
        .any(|name| name == "manual" || name == "custom");

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
        for label in ["manual", " CuStOm "] {
            let parsed = parse_labels(&[json!({"name": label}), json!({"name": "rewarded"})], &[]);
            assert!(parsed.is_manual && parsed.is_rewarded);
            assert_eq!(parsed.matched_level.as_deref(), Some("manual"));
        }
    }

    #[test]
    fn reward_label_aliases_work_with_or_without_a_price_label() {
        for name in ["ReWaRdEd", " TrUsTlEsS-OsS ", " BoUnTy "] {
            let mut labels = vec![json!({"name": name})];
            assert!(parse_labels(&labels, &["low".into()]).is_rewarded);
            labels.push(json!({"name": "low"}));
            let parsed = parse_labels(&labels, &["low".into()]);
            assert!(parsed.is_rewarded);
            assert_eq!(parsed.matched_level.as_deref(), Some("low"));
            for rejected in ["rejected", " ReJeCt "] {
                let mut rejected_labels = labels.clone();
                rejected_labels.push(json!({"name": rejected}));
                let parsed = parse_labels(&rejected_labels, &["low".into()]);
                assert!(parsed.is_rejected && !parsed.is_rewarded);
            }
        }
        assert!(!parse_labels(&[json!({"name": "manual"})], &[]).is_rewarded);
    }

    #[test]
    fn hyphens_are_preserved_in_reward_and_price_labels() {
        for level in ["low", "good-first-issue"] {
            let parsed = parse_labels(
                &[json!({"name": "trustless-oss"}), json!({"name": level})],
                &[level.into()],
            );
            assert!(parsed.is_rewarded);
            assert_eq!(parsed.matched_level.as_deref(), Some(level));
        }
        assert!(!parse_labels(&[json!({"name": "trustlessoss"})], &[]).is_rewarded);
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
