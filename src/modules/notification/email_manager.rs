use crate::modules::notification::kinds::Kind;
use serde_json::Value;

pub struct EmailTemplate {
    pub subject: String,
    pub html: String,
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn value(data: &Value, key: &str, fallback: &str) -> String {
    match data.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => fallback.to_string(),
    }
}
fn safe_base(input: &str) -> String {
    let input = input.trim_end_matches('/');
    let authority = input
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("");
    if !authority.is_empty()
        && !input.chars().any(|c| {
            c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\')
        })
        && !authority.contains('@')
        && !authority.contains('?')
        && !authority.contains('#')
        && !input.contains('?')
        && !input.contains('#')
    {
        input.to_string()
    } else {
        "https://www.trustless-oss.xyz".to_string()
    }
}
/// Validate a link for use in an `href`. Accepts only well-formed `https://`
/// URLs that contain no characters that could break out of the attribute or
/// smuggle another scheme. Anything else (including an empty string) returns
/// the provided fallback, so templates never render a blank or unsafe link.
fn safe_link(input: &str, fallback: &str) -> String {
    let candidate = input.trim();
    let has_authority = candidate
        .strip_prefix("https://")
        .map(|rest| !rest.is_empty() && !rest.starts_with('/'))
        .unwrap_or(false);
    let is_clean = !candidate.chars().any(|c| {
        c.is_control() || c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '\\' | '`')
    });
    if has_authority && is_clean {
        candidate.to_string()
    } else {
        fallback.to_string()
    }
}
pub fn email_template_for(
    kind: Kind,
    title: &str,
    body: &str,
    data: &Value,
    app_url: &str,
) -> EmailTemplate {
    let (source, subject) = match kind {
        Kind::BountyAssigned => (BOUNTYASSIGNED, "You've been assigned a bounty"),
        Kind::BountyUnassigned => (BOUNTYUNASSIGNED, "Your bounty assignment was removed"),
        Kind::BountyLocked => (BOUNTYLOCKED, "Bounty reward secured in escrow"),
        Kind::BountyCancelled => (BOUNTYCANCELLED, "This bounty has been cancelled"),
        Kind::BountyRejected => (BOUNTYREJECTED, "Your bounty submission needs changes"),
        Kind::AmountUpdated => (AMOUNTUPDATED, "Bounty reward amount updated"),
        Kind::AmountMissing => (AMOUNTMISSING, "Action needed: set the bounty reward"),
        Kind::WalletRequired => (
            WALLETREQUIRED,
            "Action needed: connect your wallet to get paid",
        ),
        Kind::PayoutReleased => (PAYOUTRELEASED, "Your bounty payout has been sent"),
        Kind::PayoutBlocked => (PAYOUTBLOCKED, "Action needed: bounty payout is blocked"),
        Kind::InsufficientFunds => (
            INSUFFICIENTFUNDS,
            "Action needed: escrow has insufficient funds",
        ),
        Kind::EscrowFunded => (ESCROWFUNDED, "Bounty escrow has been funded"),
        Kind::EscrowRefunded => (ESCROWREFUNDED, "Bounty escrow funds were refunded"),
    };
    let base = safe_base(app_url);
    let name = value(data, "name", "there");
    let username = value(data, "username", "");
    let repo = value(data, "repository", "See repository in your account");
    let issue = value(data, "issue", "—");
    let amount = value(data, "amount", "Not provided");
    let asset = value(data, "asset", &value(data, "currency", ""));
    let fallback_link = format!("{base}/bounties");
    let repo_url = safe_link(&value(data, "repo_url", ""), &fallback_link);
    let issue_url = safe_link(&value(data, "issue_url", ""), &fallback_link);
    let replacements = [
        ("TOSS_NAME_TOKEN", escape_html(&name)),
        ("TOSS_USERNAME_TOKEN", escape_html(&username)),
        ("TOSS_REPO_TOKEN", escape_html(&repo)),
        ("TOSS_ISSUE_TOKEN", escape_html(&issue)),
        ("TOSS_AMOUNT_TOKEN", escape_html(&amount)),
        ("TOSS_ASSET_TOKEN", escape_html(&asset)),
        ("TOSS_REPO_URL_TOKEN", escape_html(&repo_url)),
        ("TOSS_ISSUE_URL_TOKEN", escape_html(&issue_url)),
        ("TOSS_TITLE_TOKEN", escape_html(title)),
        ("TOSS_BODY_TOKEN", escape_html(body).replace('\n', "<br>")),
    ];
    let original = source.replace("https://www.trustless-oss.xyz", &escape_html(&base));
    let mut html = String::with_capacity(original.len());
    let mut rest = original.as_str();
    while !rest.is_empty() {
        if let Some((position, token, replacement)) = replacements
            .iter()
            .filter_map(|(token, replacement)| rest.find(token).map(|p| (p, *token, replacement)))
            .min_by_key(|(position, _, _)| *position)
        {
            html.push_str(&rest[..position]);
            html.push_str(replacement);
            rest = &rest[position + token.len()..];
        } else {
            html.push_str(rest);
            break;
        }
    }
    let title = title.replace(['\r', '\n'], " ");
    let subject = if title.trim().is_empty() {
        subject.to_string()
    } else {
        format!("{} · {}", subject, title)
    };
    EmailTemplate { subject, html }
}

// ---------------------------------------------------------------------------
// Template HTML bodies.
//
// Each template is stored in its own editable file under
// `src/infra/email/templates/` and embedded here at compile time. To change
// an email's markup, edit the corresponding `.html` file — nothing in this
// Rust file needs to change.
// ---------------------------------------------------------------------------
const BOUNTYASSIGNED: &str = include_str!("../../infra/email/templates/bounty_assigned.html");
const BOUNTYUNASSIGNED: &str = include_str!("../../infra/email/templates/bounty_unassigned.html");
const BOUNTYLOCKED: &str = include_str!("../../infra/email/templates/bounty_locked.html");
const BOUNTYCANCELLED: &str = include_str!("../../infra/email/templates/bounty_cancelled.html");
const BOUNTYREJECTED: &str = include_str!("../../infra/email/templates/bounty_rejected.html");
const AMOUNTUPDATED: &str = include_str!("../../infra/email/templates/amount_updated.html");
const AMOUNTMISSING: &str = include_str!("../../infra/email/templates/amount_missing.html");
const WALLETREQUIRED: &str = include_str!("../../infra/email/templates/wallet_required.html");
const PAYOUTRELEASED: &str = include_str!("../../infra/email/templates/payout_released.html");
const PAYOUTBLOCKED: &str = include_str!("../../infra/email/templates/payout_blocked.html");
const INSUFFICIENTFUNDS: &str = include_str!("../../infra/email/templates/insufficient_funds.html");
const ESCROWFUNDED: &str = include_str!("../../infra/email/templates/escrow_funded.html");
const ESCROWREFUNDED: &str = include_str!("../../infra/email/templates/escrow_refunded.html");

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escapes_untrusted_text_and_blocks_header_injection() {
        let t = email_template_for(
            Kind::BountyAssigned,
            "<script>\r\nBcc: bad",
            "<img src=x onerror=evil>",
            &serde_json::json!({"name":"<Alex>"}),
            "javascript:alert(1)",
        );
        assert!(!t.html.contains("<script>"));
        assert!(!t.html.contains("onerror=evil>"));
        assert!(t.html.contains("&lt;Alex&gt;"));
        assert!(!t.subject.contains('\n'));
        assert!(t.html.contains("https://www.trustless-oss.xyz/bounties"));
    }
    #[test]
    fn replaces_data_once() {
        let t = email_template_for(
            Kind::PayoutReleased,
            "Contribution",
            "TOSS_NAME_TOKEN",
            &serde_json::json!({"name":"TOSS_REPO_TOKEN", "amount":"72.50","asset":"USDC"}),
            "https://app.example.com/",
        );
        // A value that happens to contain another token's name must not be
        // expanded a second time: "name" was set to the literal
        // "TOSS_REPO_TOKEN", so it should appear verbatim in the greeting.
        assert!(t.html.contains("Hi TOSS_REPO_TOKEN (@"));
        assert!(t.html.contains("72.50"));
        assert!(t.html.contains("USDC"));
        assert!(t.html.contains("https://app.example.com/dashboard"));
    }

    #[test]
    fn addresses_recipient_by_name_and_github_handle() {
        let t = email_template_for(
            Kind::BountyAssigned,
            "Fix the login bug",
            "Please open a PR when ready.",
            &serde_json::json!({
                "name": "Ada Lovelace",
                "username": "ada",
                "repository": "trustless-oss/app",
                "issue": "42",
            }),
            "https://app.example.com",
        );
        // The greeting names the person and shows their GitHub handle.
        assert!(t.html.contains("Hi Ada Lovelace (@ada),"));
        // The footer states clearly why they received it, naming the handle.
        assert!(t.html.contains("@ada"));
        assert!(t.html.contains("You are receiving it because"));
    }
    #[test]
    fn renders_real_repo_and_issue_links() {
        let t = email_template_for(
            Kind::BountyAssigned,
            "Fix login",
            "Open a PR.",
            &serde_json::json!({
                "name": "Ada",
                "username": "ada",
                "repository": "trustless-oss/app",
                "issue": "42",
                "repo_url": "https://github.com/trustless-oss/app",
                "issue_url": "https://github.com/trustless-oss/app/issues/42",
            }),
            "https://app.example.com",
        );
        assert!(t
            .html
            .contains("href=\"https://github.com/trustless-oss/app/issues/42\""));
        assert!(t
            .html
            .contains("href=\"https://github.com/trustless-oss/app\""));
        // No empty hrefs anywhere.
        assert!(!t.html.contains("href=\"\""));
    }

    #[test]
    fn missing_links_fall_back_and_are_never_blank() {
        let t = email_template_for(
            Kind::EscrowFunded,
            "Funded",
            "Details",
            &serde_json::json!({"name": "Ada", "username": "ada"}),
            "https://app.example.com",
        );
        // No link token is left unreplaced and no href is empty.
        assert!(!t.html.contains("TOSS_REPO_URL_TOKEN"));
        assert!(!t.html.contains("TOSS_ISSUE_URL_TOKEN"));
        assert!(!t.html.contains("href=\"\""));
        // Falls back to the app's bounties page.
        assert!(t.html.contains("https://app.example.com/bounties"));
    }

    #[test]
    fn footer_shows_support_and_navigation_links() {
        let t = email_template_for(
            Kind::BountyAssigned,
            "Fix login",
            "Open a PR.",
            &serde_json::json!({"name": "Ada", "username": "ada"}),
            "https://app.example.com",
        );
        assert!(t.html.contains("Need help?"));
        assert!(t.html.contains(">Dashboard</a>"));
        assert!(t.html.contains(">Notification Settings</a>"));
        assert!(t.html.contains(">Bounties</a>"));
        assert!(t.html.contains("Open Source, Trustless Rewards"));
    }

    #[test]
    fn does_not_invent_missing_amount() {
        let t = email_template_for(
            Kind::EscrowFunded,
            "Funded",
            "Details",
            &serde_json::json!({}),
            "https://app.example.com",
        );
        assert!(t.html.contains("Not provided"));
        assert!(!t.html.contains("500.00"));
    }
}
