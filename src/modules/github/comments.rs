use rust_decimal::Decimal;

// Use native GitHub Markdown and literal newlines so the source matches the comment layout.

// ─────────────────────────────────────────────────────────────
// Static messages
// ─────────────────────────────────────────────────────────────

pub const ESCROW_SETUP_REQUIRED: &str = r#"**🏦 Escrow setup required**

Your reward request is saved. The repository escrow must be deployed and funded before funds can be locked or paid out.

> [!IMPORTANT]
> **Next step:** Deploy and fund the escrow from your dashboard.
> Processing resumes automatically when ready."#;

pub const LINKED_ISSUE_REQUIRED: &str = r#"**🔗 Linked issue required**

Link the issue being rewarded before setting a reward on this pull request.

> [!TIP]
> **Next step:** Add `Closes #123` to the PR description using your issue number. Retry the reward command."#;

pub const HELP: &str = r#"**💬 Reward commands**

Comment on an issue or linked pull request. Reward and payment commands require maintainer access.

<details>
<summary>💬 Commands</summary>

| Command | Action | Who can use it |
| :--- | :--- | :--- |
| `@trustless-oss 100 USDC` | Set a **100 USDC** reward | Maintainers |
| `@trustless-oss pay 70%` | Set payment to **70%** | Maintainers |
| `@trustless-oss pay 70` | Set payment to **70 USDC** | Maintainers |
| `@trustless-oss reject` | Reject work; return reward to pool | Maintainers |
| `@trustless-oss retry` | Recheck eligibility | Maintainers |
| `@trustless-oss wallet` | Connect or update your wallet | Anyone |
| `@trustless-oss help` | Show this guide | Anyone |

</details>

> [!NOTE]
> Without `%`, amounts are USDC. Payment is automatic after an eligible linked PR merges and payout requirements are met."#;

pub const BOUNTY_UPDATE_FAILED: &str = r#"**🏦 Reward increase awaiting funding**

Your requested increase is saved, but the escrow cannot cover it yet. The current reward amount remains unchanged.

> [!TIP]
> **Next step:** Check the dashboard shortfall and fund the escrow. Recheck is automatic."#;

pub const INVALID_SPLIT: &str = r#"**⚠️ Invalid payment amount**

The contributor payment must be **greater than zero** and **no more than the reward amount**. Your current payment is unchanged.

> [!TIP]
> **Next step:** Use `@trustless-oss pay 70%` for a percentage or `@trustless-oss pay 70` for **70 USDC**."#;

pub const MANUAL_AMOUNT_NEEDED: &str = r#"**💰 Reward amount required**

A contributor can be assigned, but this issue needs a reward amount before funds can be locked.

> [!IMPORTANT]
> **Next step:** Comment `@trustless-oss 100 USDC`, replacing `100` with your chosen amount."#;

pub const RETRYING_BOUNTY: &str = r#"**🔄 Reward eligibility check queued**

We will check this issue again and continue reward processing if it is eligible.

> [!NOTE]
> No repeat command is needed. A missing reward amount, disconnected wallet, or unmerged PR may still need your attention."#;

pub const UNPRICED_BOUNTY_CREATED: &str = r#"**💰 Reward awaiting an amount**

This issue has no reward amount yet.

> [!TIP]
> **Next step:** Add a configured reward label or comment `@trustless-oss 100 USDC` with your chosen amount."#;

// ─────────────────────────────────────────────────────────────
// Wallet
// ─────────────────────────────────────────────────────────────

pub fn wallet_update(login: &str, connect_url: &str) -> String {
    format!(
        r#"**👛 Connect your wallet**

Hi @{login},

Connect a Stellar wallet you control that accepts USDC on the configured network. Never share secret keys or recovery phrases.

> [!TIP]
> **Next step:** [Open your profile]({connect_url}). Processing resumes automatically."#
    )
}

pub fn maintainer_wallet_required(login: &str, app_url: &str) -> String {
    format!(
        r#"**👛 Maintainer wallet required**

Hi @{login},

Connect your Stellar wallet through your profile. Posting an address here does not connect it.

> [!IMPORTANT]
> **Next step:** [Connect your wallet]({app_url}/profile) before continuing."#
    )
}

pub fn contributor_wallet_missing(username: &str) -> String {
    format!(
        r#"**👛 Contributor wallet required**

Hi @{username},

Connect your Stellar wallet through your profile so a maintainer can configure your payment.

> [!TIP]
> **Next step:** @{username}, comment `@trustless-oss wallet` to get your profile link."#
    )
}

// ─────────────────────────────────────────────────────────────
// Reward created / updated
// ─────────────────────────────────────────────────────────────

fn reward_created_next_step(has_assignee: bool) -> &'static str {
    if has_assignee {
        "A contributor is already assigned. The bot will check their connected wallet and escrow requirements automatically; watch for the escrow confirmation or any required action."
    } else {
        "Assign a contributor with a connected wallet to lock the reward."
    }
}

pub fn manual_bounty_created(
    reward: Decimal,
    contract_id: &str,
    creator: &str,
    has_assignee: bool,
) -> String {
    let next_step = reward_created_next_step(has_assignee);
    format!(
        r#"**💰 Reward created**

Hi @{creator},

Your reward has been added to this issue. Funds will be locked for an eligible assigned contributor; no payment has been made.

**Reward:** {reward} USDC · [View escrow](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> **Next step:** {next_step}"#
    )
}

pub fn labeled_bounty_created(
    reward: Decimal,
    difficulty: &str,
    contract_id: &str,
    has_assignee: bool,
) -> String {
    let next_step = reward_created_next_step(has_assignee);
    format!(
        r#"**💰 Reward created**

This reward was set from the issue’s configured label. Escrow locking is confirmed separately; creating a reward does not make a payment.

**Reward:** {reward} USDC · **Label:** `{difficulty}`

[View escrow](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> **Next step:** {next_step} You can change the amount before it is locked."#
    )
}

pub fn manual_bounty_updated(amount: Decimal) -> String {
    format!(
        r#"**💰 Reward amount updated**

The reward for this issue is now **{amount} USDC**. This updates the offered amount; it does not release payment.

> [!NOTE]
> Funds lock when the assigned contributor meets the requirements."#
    )
}

// ─────────────────────────────────────────────────────────────
// Balance problems
// ─────────────────────────────────────────────────────────────

pub(super) fn funding_notice(body: &str) -> Option<&str> {
    body.lines().next().filter(|heading| {
        matches!(
            *heading,
            "**🏦 Reward awaiting funding**"
                | "**🏦 Reward increase awaiting funding**"
                | "**🏦 Escrow setup required**"
        )
    })
}

pub fn manual_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    insufficient_balance(balance, reward, app_url)
}

pub fn labeled_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    insufficient_balance(balance, reward, app_url)
}

fn insufficient_balance(_balance: Decimal, reward: Decimal, app_url: &str) -> String {
    format!(
        r#"**🏦 Reward awaiting funding**

Insufficient escrow funds. The amount below is the total reward, not the required deposit. Check the dashboard for the shortfall.

**Requested reward:** {reward} USDC

> [!TIP]
> **Next step:** [Fund the escrow]({app_url}/dashboard) to resume automatically.
> Your request is saved; no retry command is needed."#
    )
}

// ─────────────────────────────────────────────────────────────
// Assignment
// ─────────────────────────────────────────────────────────────

pub fn contributor_assigned_without_amount(login: &str) -> String {
    format!(
        r#"**📌 Contributor assigned — reward amount required**

@{login} is assigned to this issue. **No funds are locked yet.** A maintainer must set the reward amount to continue.

> [!IMPORTANT]
> **Next step:** Set a reward label or comment `@trustless-oss 100 USDC` with your amount. Locking resumes automatically."#
    )
}

pub fn contributor_unassigned(reward: Decimal) -> String {
    format!(
        r#"**📌 Reward ready for reassignment**

The previous incomplete milestone was disputed and rejected. Its **{reward} USDC** allocation remains in the repository escrow pool.

> [!TIP]
> **Next step:** Assign another contributor."#
    )
}

// ─────────────────────────────────────────────────────────────
// Payout
// ─────────────────────────────────────────────────────────────

pub fn payout_intent_saved(contributor_amount: Decimal, contributor_username: &str) -> String {
    format!(
        r#"**📝 Payment amount saved**

Hi @{contributor_username},

Your payment is set to **{contributor_amount} USDC**. Funds have not been released. The linked PR must be eligible and authored by the assigned contributor.

> [!NOTE]
> Payment eligibility is checked after the linked PR merges. If it is already merged, we will check now."#,
    )
}

pub fn pr_author_mismatch(issue_number: i32) -> String {
    format!(
        r#"**⚠️ Payment unchanged — contributor mismatch**

The PR author does not match the contributor assigned to issue #{issue_number}. **No payment change was made.**

> [!TIP]
> **Next step:** Check the assignee and PR link, then retry."#
    )
}

pub fn payout_aborted_for_pr_author(issue_number: i32) -> String {
    format!(
        r#"**⚠️ Payment blocked — contributor mismatch**

The merged PR was not authored by the contributor assigned to issue #{issue_number}. **No payment was released.**

> [!IMPORTANT]
> **Next step:** Maintainer: review the assignee and PR author."#
    )
}

// ─────────────────────────────────────────────────────────────
// Cancelled / rejected
// ─────────────────────────────────────────────────────────────

pub fn bounty_cancelled(reward: Decimal) -> String {
    format!(
        r#"**🚫 Reward cancelled**

**{reward} USDC** is available again in the reward pool.

> [!WARNING]
> This reward is no longer active. No further payment will be made for this work."#
    )
}

pub fn bounty_rejected(reward: Decimal, contract_id: &str) -> String {
    format!(
        r#"**🚫 Work rejected — reward returned**

The work was rejected. Its **{reward} USDC** allocation has returned to the repository reward pool.

[View escrow](https://viewer.trustlesswork.com/{contract_id})"#
    )
}

pub fn bounty_locked(reward: Decimal, username: &str, contract_id: &str) -> String {
    format!(
        r#"**🔒 Reward secured in escrow**

Hi @{username},

**{reward} USDC** is reserved for you. The funds are held in escrow and have not yet been paid to the contributor.

[View escrow](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> **Next step:** Submit a linked PR for maintainer review. Payment processing continues automatically when an eligible PR is merged."#
    )
}

pub fn payout_released(reward: Decimal, username: &str, explorer_url: &str) -> String {
    format!(
        r#"**✅ Reward payment completed**

Congratulations, @{username}! 🎉

**{reward} USDC** has been released to you. View the transaction below to inspect the payment.

Thank you for contributing.

[View transaction]({explorer_url})"#
    )
}

pub fn split_payout_released(
    contributor_amount: Decimal,
    maintainer_amount: Decimal,
    username: &str,
    contract_id: &str,
) -> String {
    format!(
        r#"**✅ Partial payment completed**

Congratulations, @{username}! 🎉

Your partial payment is complete. The remaining allocation returned to the reward pool.

| Allocation | Amount |
| :--- | ---: |
| Payment to @{username} | **{contributor_amount} USDC** |
| Refunded to Pool | **{maintainer_amount} USDC** |

[View escrow](https://viewer.trustlesswork.com/{contract_id})

Thank you, @{username}."#
    )
}

pub fn split_payout_waiting_for_wallet(app_url: &str) -> String {
    format!(
        r#"**👛 Partial payment awaiting a payout wallet**

The partial payment is ready, but the repository payout wallet must be connected to complete the distribution. **No funds have been released.**

> [!IMPORTANT]
> **Next step:** [Connect the repository payout wallet]({app_url}/profile) to continue."#
    )
}

pub fn payout_waiting_for_wallet(reward: Decimal, username: &str, connect_url: &str) -> String {
    format!(
        r#"**👛 Wallet required to receive your reward**

Hi @{username},

Your **{reward} USDC** reward needs a connected Stellar wallet. Connecting a wallet alone does not release funds; an eligible linked PR must also be merged.

> [!IMPORTANT]
> **Next step:** [Connect your wallet]({connect_url}). Processing resumes automatically after wallet and merge requirements are met."#
    )
}

pub fn bounty_cancelled_after_refund(contract_id: &str) -> String {
    format!(
        r#"**🚫 Reward closed — escrow funds withdrawn**

A maintainer withdrew the escrowed funds.

> [!WARNING]
> This reward is closed. Further work on this issue is not eligible for payment under this reward.

[View escrow](https://viewer.trustlesswork.com/{contract_id})"#
    )
}

/// Address the person responsible for an action without accepting arbitrary mentions.
pub fn addressed_to(login: Option<&str>, body: &str) -> String {
    match login.filter(|login| {
        !login.is_empty() && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }) {
        Some(login) => match body.split_once("\n\n") {
            Some((heading, content))
                if heading.starts_with('#')
                    || (heading.starts_with("**") && heading.ends_with("**")) =>
            {
                format!("{heading}\n\nHi @{login},\n\n{content}")
            }
            _ => format!("Hi @{login},\n\n{body}"),
        },
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reward_created_guidance_matches_current_assignment() {
        for has_assignee in [false, true] {
            for body in [
                super::labeled_bounty_created(
                    rust_decimal::Decimal::ONE,
                    "bug",
                    "escrow",
                    has_assignee,
                ),
                super::manual_bounty_created(
                    rust_decimal::Decimal::ONE,
                    "escrow",
                    "owner",
                    has_assignee,
                ),
            ] {
                assert_eq!(
                    body.contains("A contributor is already assigned"),
                    has_assignee
                );
                assert_eq!(body.contains("Assign a contributor"), !has_assignee);
                assert!(body.contains("1 USDC"));
                assert!(body.contains("https://viewer.trustlesswork.com/escrow"));
                assert!(!body.contains("Reward secured"));
            }
        }
    }

    use super::*;

    #[test]
    fn addressed_comments_keep_the_title_and_validate_mentions() {
        let body = addressed_to(Some("maintainer-1"), ESCROW_SETUP_REQUIRED);
        assert!(body.starts_with("**🏦 Escrow setup required**\n\nHi @maintainer-1,\n\n"));
        assert!(!body.contains("<details>"));
        assert!(body.contains("> [!IMPORTANT]\n> **Next step:**"));
        assert_eq!(addressed_to(Some("bad\n@mention"), HELP), HELP);
        assert_eq!(addressed_to(None, HELP), HELP);
        assert_eq!(
            addressed_to(Some("maintainer"), "### Status\n\nMessage"),
            "### Status\n\nHi @maintainer,\n\nMessage"
        );
        assert_eq!(
            addressed_to(Some("maintainer"), "Message"),
            "Hi @maintainer,\n\nMessage"
        );
    }

    #[test]
    fn payment_comments_preserve_amounts_links_and_balance_privacy() {
        let balance = Decimal::from(987);
        let reward = Decimal::from(125);
        let manual = manual_insufficient_balance(balance, reward, "https://app.example");
        assert_eq!(
            manual,
            labeled_insufficient_balance(balance, reward, "https://app.example")
        );
        assert!(!manual.contains("987"));
        assert!(manual.contains("125 USDC"));
        assert!(manual.contains("**Requested reward:** 125 USDC"));
        assert!(!manual.contains('|'));
        assert!(manual.contains("[Fund the escrow](https://app.example/dashboard)"));
        let addressed = addressed_to(Some("repo-owner"), &manual);
        assert!(addressed.contains("Hi @repo-owner,\n\n"));
        assert!(!addressed.contains("<details>"));
        assert!(manual.contains("no retry command is needed"));
        let payout = payout_intent_saved(reward, "contributor");
        assert!(payout.contains("Payment amount saved"));
        assert!(payout.contains("125 USDC"));
        assert!(payout.contains("Hi @contributor,\n\n"));
        let split = split_payout_released(reward, balance, "contributor", "C_ESCROW");
        assert!(split.contains("Congratulations, @contributor!"));
        let released = payout_released(reward, "contributor", "https://explorer.example/tx");
        assert!(released.contains("Congratulations, @contributor!"));
        assert!(released.contains("125 USDC"));
        assert!(released.contains("https://explorer.example/tx"));
        assert!(split.contains("@contributor"));
        assert!(split.contains("125 USDC"));
        assert!(split.contains("987 USDC"));
        assert!(split.contains("Refunded to Pool"));
        assert!(!split.contains("maintainer"));
        assert!(split.contains("https://viewer.trustlesswork.com/C_ESCROW"));
    }
}
