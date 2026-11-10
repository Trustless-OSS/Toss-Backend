use rust_decimal::Decimal;

// Use native GitHub Markdown and literal newlines so the source matches the comment layout.

// ─────────────────────────────────────────────────────────────
// Static messages
// ─────────────────────────────────────────────────────────────

pub const ESCROW_SETUP_REQUIRED: &str = r#"### Reward setup required

Your reward request is saved. The repository escrow is not ready yet, so the reward cannot be locked or paid out. Setting a reward amount does not transfer funds by itself.

> [!IMPORTANT]
> **Next step:** Deploy and fund the escrow from your dashboard.
> Reward processing will resume when the escrow is ready."#;

pub const LINKED_ISSUE_REQUIRED: &str = r#"### Link an issue to continue

This pull request needs a linked issue so we can track its reward against the assigned contributor. Use the issue that describes the work being rewarded.

> [!TIP]
> **Next step:** Add `Closes #123` to the pull request description, replacing `123` with the issue number. Then set the reward again."#;

pub const HELP: &str = r#"### Reward command guide

Mention `@trustless-oss` in an issue or pull request comment to get started. Reward and payment changes require maintainer access. When commenting on a pull request, link the relevant issue in its description first.

| Command | Action | Who can use it |
| :--- | :--- | :--- |
| `@trustless-oss 100 USDC` | Set a **100 USDC** reward | Maintainers |
| `@trustless-oss pay 70%` | Set the contributor payment to **70%** of the reward | Maintainers |
| `@trustless-oss pay 70` | Set the contributor payment to **70 USDC** | Maintainers |
| `@trustless-oss reject` | Reject the work and return the escrowed reward | Maintainers |
| `@trustless-oss retry` | Request a fresh reward eligibility check | Maintainers |
| `@trustless-oss wallet` | Connect or update your wallet | Anyone |
| `@trustless-oss help` | Show this guide | Anyone |

> [!NOTE]
> Amounts without `%` are fixed USDC amounts. Payments proceed automatically once an eligible linked pull request is merged and the payout requirements are met."#;

pub const BOUNTY_UPDATE_FAILED: &str = r#"### Reward update needs funds

The escrow cannot cover the requested increase. The current reward amount is unchanged; the higher amount has not taken effect. Check the saved request and available funds in the dashboard before topping up.

> [!TIP]
> **Next step:** Add funds to the repository escrow. Your requested amount is saved and will be checked automatically."#;

pub const INVALID_SPLIT: &str = r#"### Payment amount not accepted

The contributor payment must be **greater than zero** and **no more than the reward amount**. The current payment is unchanged.

> [!TIP]
> **Next step:** Use `@trustless-oss pay 70%` for a percentage or `@trustless-oss pay 70` for **70 USDC**."#;

pub const MANUAL_AMOUNT_NEEDED: &str = r#"### Set a reward amount

A contributor can be assigned, but no funds can be locked until this issue has a reward amount.

> [!IMPORTANT]
> **Next step:** Comment `@trustless-oss 100 USDC`, replacing `100` with your chosen amount."#;

pub const RETRYING_BOUNTY: &str = r#"### Reward check queued

We will check this issue again and continue reward processing if it is eligible.

> [!NOTE]
> No repeat command is needed. An unset amount, missing wallet, or unmerged pull request may still require action."#;

pub const UNPRICED_BOUNTY_CREATED: &str = r#"### Reward amount pending

This issue is marked for a reward, but no amount has been set.

> [!TIP]
> **Next step:** Add a configured reward label or comment `@trustless-oss 100 USDC` with your chosen amount."#;

// ─────────────────────────────────────────────────────────────
// Wallet
// ─────────────────────────────────────────────────────────────

pub fn wallet_update(login: &str, connect_url: &str) -> String {
    format!(
        r#"### Connect your Stellar wallet

@{login}, add or update the Stellar wallet on your profile to receive rewards. Use an account you control that can receive USDC on the configured Stellar network. Never post a secret key or recovery phrase in this thread.

> [!TIP]
> **Next step:** [Open your profile]({connect_url}). Payment processing can continue automatically once your wallet is connected."#
    )
}

pub fn maintainer_wallet_required(login: &str, app_url: &str) -> String {
    format!(
        r#"### Maintainer wallet required

@{login}, this payment action needs a connected Stellar wallet. Add the wallet through your profile so it is associated with your account; posting an address here does not connect it.

> [!IMPORTANT]
> **Next step:** [Connect your wallet]({app_url}/profile) before continuing."#
    )
}

pub fn contributor_wallet_missing(username: &str) -> String {
    format!(
        r#"### Contributor wallet required

The payment cannot be configured until @{username} connects a Stellar wallet through their profile. After the wallet is connected, a maintainer can submit the payment command again.

> [!TIP]
> **Next step:** @{username}, comment `@trustless-oss wallet` to get your profile link."#
    )
}

// ─────────────────────────────────────────────────────────────
// Reward created / updated
// ─────────────────────────────────────────────────────────────

pub fn manual_bounty_created(reward: Decimal, contract_id: &str, creator: &str) -> String {
    format!(
        r#"### Reward created

@{creator} added a reward to this issue. This records the amount offered for the work; it does not mean a contributor has been paid. A separate confirmation will be posted when funds are locked.

| Reward | Escrow |
| ---: | :--- |
| **{reward} USDC** | [View escrow](https://viewer.trustlesswork.com/{contract_id}) |

> [!TIP]
> **Next step:** Assign a contributor with a connected wallet to lock the reward."#
    )
}

pub fn labeled_bounty_created(reward: Decimal, difficulty: &str, contract_id: &str) -> String {
    format!(
        r#"### Reward created

This issue now has a reward based on its configured label. The amount below is the reward offered for the work. Funds still need to be locked for an assigned contributor before payment can proceed.

| Reward | Label | Escrow |
| ---: | :--- | :--- |
| **{reward} USDC** | `{difficulty}` | [View escrow](https://viewer.trustlesswork.com/{contract_id}) |

> [!TIP]
> **Next step:** Assign a contributor with a connected wallet to lock the reward. You can change the amount before it is locked."#
    )
}

pub fn manual_bounty_updated(amount: Decimal) -> String {
    format!(
        r#"### Reward updated

The reward for this issue is now **{amount} USDC**. This updates the offered amount; it is not a payment confirmation.

> [!NOTE]
> Funds will be locked in escrow when the assigned contributor meets the requirements."#
    )
}

// ─────────────────────────────────────────────────────────────
// Balance problems
// ─────────────────────────────────────────────────────────────

pub fn manual_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    insufficient_balance(balance, reward, app_url)
}

pub fn labeled_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    insufficient_balance(balance, reward, app_url)
}

fn insufficient_balance(_balance: Decimal, reward: Decimal, app_url: &str) -> String {
    format!(
        r#"### Reward needs funding

The escrow cannot cover the requested reward yet. The amount below is the full requested reward, not the additional deposit required. Check the dashboard for the available balance and the amount you need to add.

| Required reward |
| ---: |
| **{reward} USDC** |

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
        r#"### Assignment saved: reward amount pending

@{login} is assigned to this issue. **No funds are locked yet.** A maintainer must set the reward amount before the assignment can proceed to escrow locking.

> [!IMPORTANT]
> **Next step:** Add a configured reward label or comment `@trustless-oss 100 USDC` with your chosen amount. Escrow locking will then continue automatically."#
    )
}

pub fn contributor_unassigned(reward: Decimal) -> String {
    format!(
        r#"### Reward available for reassignment

The previous incomplete milestone was disputed and rejected. Its **{reward} USDC** allocation remains in the repository escrow pool.

> [!TIP]
> **Next step:** Assign another contributor to continue work on this rewarded issue."#
    )
}

// ─────────────────────────────────────────────────────────────
// Payout
// ─────────────────────────────────────────────────────────────

pub fn payout_intent_saved(contributor_amount: Decimal, contributor_username: &str) -> String {
    format!(
        r#"### Payment amount saved

@{contributor_username}, your payment is set to **{contributor_amount} USDC**. This saves the payment instruction; it does not confirm that funds have been transferred. The linked pull request must be eligible and authored by the assigned contributor.

> [!NOTE]
> Payment eligibility will be checked after the linked pull request is merged. If it is already merged, we will check now."#,
    )
}

pub fn pr_author_mismatch(issue_number: i32) -> String {
    format!(
        r#"### Payment unchanged: contributor mismatch

The pull request author does not match the contributor assigned to issue #{issue_number}. **No payment change was made.**

> [!TIP]
> **Next step:** Check the issue assignment and pull request link before trying again."#
    )
}

pub fn payout_aborted_for_pr_author(issue_number: i32) -> String {
    format!(
        r#"### Payment blocked: contributor mismatch

The merged pull request was not authored by the contributor assigned to issue #{issue_number}. **No payment was released.**

> [!IMPORTANT]
> **Next step:** A maintainer must review the issue assignment and pull request author."#
    )
}

// ─────────────────────────────────────────────────────────────
// Cancelled / rejected
// ─────────────────────────────────────────────────────────────

pub fn bounty_cancelled(reward: Decimal) -> String {
    format!(
        r#"### Reward cancelled

The **{reward} USDC** allocation is available again in the repository reward pool.

> [!WARNING]
> This reward is no longer active. No further payment will be made for this work."#
    )
}

pub fn bounty_rejected(reward: Decimal, contract_id: &str) -> String {
    format!(
        r#"### Work rejected

The work was rejected. The **{reward} USDC** allocation was returned to the repository reward pool and is available for future rewards.

[View escrow](https://viewer.trustlesswork.com/{contract_id})"#
    )
}

pub fn bounty_locked(reward: Decimal, username: &str, contract_id: &str) -> String {
    format!(
        r#"### Reward locked in escrow

**{reward} USDC** is reserved for @{username}. The funds are held in escrow and have not yet been transferred to the contributor. You can inspect the escrow using the link below.

[View escrow](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> **Next step:** Merge an eligible linked pull request authored by the assigned contributor when the work is complete. Payment processing will continue automatically."#
    )
}

pub fn payout_released(reward: Decimal, username: &str, explorer_url: &str) -> String {
    format!(
        r#"### Reward payment released

@{username}, **{reward} USDC** has been released to you. Use the transaction link below to inspect the payment details. No further payment command is needed for this reward.

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
        r#"### Partial payment released

The partial payment has been released. The table below shows the contributor payment and the remaining amount distributed according to the repository payout settings. No further payment command is needed for this distribution.

| Allocation | Amount |
| :--- | ---: |
| Payment to @{username} | **{contributor_amount} USDC** |
| Refunded to Pool | **{maintainer_amount} USDC** |

[View escrow](https://viewer.trustlesswork.com/{contract_id})

Thank you for your contribution, @{username}."#
    )
}

pub fn split_payout_waiting_for_wallet(app_url: &str) -> String {
    format!(
        r#"### Partial payment waiting for a payout wallet

The partial payment is ready, but the repository payout wallet needs to be connected before the distribution can be completed. No funds have been released yet.

> [!IMPORTANT]
> **Next step:** [Connect the repository payout wallet]({app_url}/profile) to continue."#
    )
}

pub fn payout_waiting_for_wallet(reward: Decimal, username: &str, connect_url: &str) -> String {
    format!(
        r#"### Connect a wallet to receive your reward

@{username}, your **{reward} USDC** reward needs a connected Stellar wallet before payment can proceed. Connect it through your profile using the link below. Connecting a wallet alone does not release funds; the work must also meet the payment requirements.

> [!IMPORTANT]
> **Next step:** [Connect your wallet]({connect_url}). Processing resumes automatically; payment requires an eligible linked pull request to be merged."#
    )
}

pub fn bounty_cancelled_after_refund(contract_id: &str) -> String {
    format!(
        r#"### Reward cancelled: funds withdrawn

A repository maintainer withdrew the escrowed funds.

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
            Some((heading, content)) if heading.starts_with('#') => {
                format!("{heading}\n\n@{login}\n\n{content}")
            }
            _ => format!("@{login}\n\n{body}"),
        },
        None => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(manual.contains("no retry command is needed"));
        let payout = payout_intent_saved(reward, "contributor");
        assert!(payout.contains("Payment amount saved"));
        assert!(payout.contains("125 USDC"));
        let split = split_payout_released(reward, balance, "contributor", "C_ESCROW");
        assert!(split.contains("@contributor"));
        assert!(split.contains("125 USDC"));
        assert!(split.contains("987 USDC"));
        assert!(split.contains("Refunded to Pool"));
        assert!(!split.contains("maintainer"));
        assert!(split.contains("https://viewer.trustlesswork.com/C_ESCROW"));
    }
}
