use rust_decimal::Decimal;

// Use native GitHub Markdown and literal newlines so the source matches the comment layout.

// ─────────────────────────────────────────────────────────────
// Static messages
// ─────────────────────────────────────────────────────────────

pub const ESCROW_SETUP_REQUIRED: &str = r#"### 🏦 Escrow setup required

Your bounty request is saved.

> [!IMPORTANT]
> Deploy and fund this repository's escrow from the dashboard.
> Automation will resume once the escrow is ready."#;

pub const LINKED_ISSUE_REQUIRED: &str = r#"### 🔗 Linked issue required

> [!IMPORTANT]
> Add `Closes #123` to the PR description, then set the reward again.
> The bounty will be tracked on that issue."#;

pub const HELP: &str = r#"### 🤖 Commands

Mention `@trustless-oss` in an issue or pull request comment.

| Command | Result |
| :--- | :--- |
| `@trustless-oss 100 USDC` | Set a 100 USDC bounty · maintainers |
| `@trustless-oss pay 70%` | Set the contributor payout to 70% · maintainers |
| `@trustless-oss pay 70` | Set the contributor payout to 70 USDC · maintainers |
| `@trustless-oss reject` | Reject work and return the escrowed reward · maintainers |
| `@trustless-oss retry` | Request a fresh bounty check · maintainers |
| `@trustless-oss wallet` | Get a link to connect or update your wallet · anyone |
| `@trustless-oss help` | Show this guide · anyone |

> [!NOTE]
> A number without `%` is a fixed USDC amount. Payouts run automatically after the linked PR is merged."#;

pub const BOUNTY_UPDATE_FAILED: &str = r#"### ⚠️ Reward unchanged

The escrow balance cannot cover the new reward. The previous bounty amount is still in place.

> [!TIP]
> Top up the repository escrow. Your requested reward is saved and will be checked automatically."#;

pub const INVALID_SPLIT: &str = r#"### ⚠️ Payout unchanged

The contributor payout must be greater than zero and no more than the bounty reward.

> [!TIP]
> `@trustless-oss pay 70%` for a percentage, or `@trustless-oss pay 70` for 70 USDC."#;

pub const MANUAL_AMOUNT_NEEDED: &str = r#"### ⏳ Reward needed

This manual bounty has no amount yet. A contributor can be assigned, but funds cannot be locked until a reward is set.

> [!IMPORTANT]
> Comment `@trustless-oss 100 USDC` with your chosen amount."#;

pub const RETRYING_BOUNTY: &str = r#"### 🔄 Bounty check requested

A fresh check has been queued. If the bounty is ready, automation will continue.

No further action is needed unless the bounty is waiting on an amount, wallet, or merged PR."#;

pub const UNPRICED_BOUNTY_CREATED: &str = r#"### ⏳ Bounty awaiting reward

This issue is marked as a bounty, but it does not have a reward amount yet.

> [!IMPORTANT]
> Add one of this repository's configured reward labels, or comment `@trustless-oss 100 USDC` with your chosen amount."#;

// ─────────────────────────────────────────────────────────────
// Wallet
// ─────────────────────────────────────────────────────────────

pub fn wallet_update(login: &str, connect_url: &str) -> String {
    format!(
        r#"### 🔑 Connect your wallet

@{login}, add or update your Stellar wallet in your profile.

> [!TIP]
> [Open your profile →]({connect_url})

Your payout can continue automatically once your wallet is connected."#
    )
}

pub fn maintainer_wallet_required(login: &str, app_url: &str) -> String {
    format!(
        r#"### 🔑 Maintainer wallet needed

@{login}, connect a Stellar wallet before this payout action can continue.

> [!TIP]
> [Open your profile →]({app_url}/profile)"#
    )
}

pub fn contributor_wallet_missing(username: &str) -> String {
    format!(
        r#"### ⏳ Contributor wallet needed

A payout cannot be configured until @{username} connects a Stellar wallet.

> [!TIP]
> @{username} can comment `@trustless-oss wallet` for a profile link."#
    )
}

// ─────────────────────────────────────────────────────────────
// Bounty created / updated
// ─────────────────────────────────────────────────────────────

pub fn manual_bounty_created(reward: Decimal, contract_id: &str, creator: &str) -> String {
    format!(
        r#"### 🎯 Bounty created

@{creator} created this bounty.

- **Reward:** {reward} USDC
- **Escrow:** [View details](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> Assign a contributor and connect their wallet to lock the reward."#
    )
}

pub fn labeled_bounty_created(reward: Decimal, difficulty: &str, contract_id: &str) -> String {
    format!(
        r#"### 🎯 Bounty created

- **Reward:** {reward} USDC
- **Label:** `{difficulty}`
- **Escrow:** [View details](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> Assign a contributor and connect their wallet to lock the reward.
> You can change the amount before it is locked."#
    )
}

pub fn manual_bounty_updated(amount: Decimal) -> String {
    format!(
        r#"### 💰 Reward updated

The bounty reward is now **{amount} USDC**. The reward will be locked in escrow when the assigned contributor is ready."#
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

fn insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    format!(
        r#"### ⚠️ Escrow needs funds

This bounty cannot be created at the requested amount yet.

| Available | Required |
| ---: | ---: |
| **{balance} USDC** | **{reward} USDC** |

> [!TIP]
> [Fund the escrow]({app_url}/dashboard) to resume automatically.\
> Your request is saved; no retry command is needed."#
    )
}

// ─────────────────────────────────────────────────────────────
// Assignment
// ─────────────────────────────────────────────────────────────

pub fn contributor_assigned_without_amount(login: &str) -> String {
    format!(
        r#"### ⏳ Assignment waiting on reward

@{login} is assigned, but this bounty has no reward amount yet. No funds have been locked.

> [!IMPORTANT]
> Add a configured reward label or comment `@trustless-oss 100 USDC` with your chosen amount. Escrow locking will then continue automatically."#
    )
}

pub fn contributor_unassigned(reward: Decimal) -> String {
    format!(
        r#"### ↩️ Bounty open again

The previous assignment's milestone was closed. The **{reward} USDC** bounty remains open.

> [!TIP]
> Assign another contributor to continue."#
    )
}

// ─────────────────────────────────────────────────────────────
// Payout
// ─────────────────────────────────────────────────────────────

pub fn payout_intent_saved(contributor_amount: Decimal, contributor_username: &str) -> String {
    format!(
        r#"### 📋 Payout updated

@{contributor_username}, your payout is set to **{contributor_amount} USDC**.

> [!TIP]
> Merge the linked PR when the work is ready. If it is already merged, we will check payout eligibility now."#,
    )
}

pub fn pr_author_mismatch(issue_number: i32) -> String {
    format!(
        r#"### ⚠️ Payout unchanged

The author of this PR does not match the contributor assigned to issue #{issue_number}. No payout change was made.

> [!TIP]
> Check the GitHub assignment and linked issue before trying again."#
    )
}

pub fn payout_aborted_for_pr_author(issue_number: i32) -> String {
    format!(
        r#"### 🛑 Payout blocked

This merged PR was not authored by the contributor assigned to issue #{issue_number}. No payout was released.

> [!TIP]
> A maintainer should review the assignment and PR author."#
    )
}

// ─────────────────────────────────────────────────────────────
// Cancelled / rejected
// ─────────────────────────────────────────────────────────────

pub fn bounty_cancelled(reward: Decimal) -> String {
    format!(
        r#"### 🛑 Bounty cancelled

This bounty is no longer active. The **{reward} USDC** reward was returned to the repository pool.

No payout will be made for this bounty."#
    )
}

pub fn bounty_rejected(reward: Decimal, contract_id: &str) -> String {
    format!(
        r#"### 🛑 Work rejected

A maintainer rejected the work. The **{reward} USDC** escrowed reward was returned to the maintainer's wallet.

[View escrow →](https://viewer.trustlesswork.com/{contract_id})"#
    )
}

pub fn bounty_locked(reward: Decimal, username: &str, contract_id: &str) -> String {
    format!(
        r#"### 🔒 Reward locked

**{reward} USDC** is locked in escrow for @{username}.

[View escrow →](https://viewer.trustlesswork.com/{contract_id})

> [!TIP]
> Merge a linked PR by the assigned contributor when the work is complete. The payout will continue automatically."#
    )
}

pub fn payout_released(reward: Decimal, username: &str, explorer_url: &str) -> String {
    format!(
        r#"### ✅ Payout released

**{reward} USDC** has been released to @{username}.

[View transaction →]({explorer_url})

Thanks for contributing!"#
    )
}

pub fn split_payout_released(
    contributor_amount: Decimal,
    maintainer_amount: Decimal,
    username: &str,
    contract_id: &str,
) -> String {
    format!(
        r#"### ✅ Split payout released

The bounty payout has been distributed.

| Recipient | Amount |
| :--- | ---: |
| @{username} | **{contributor_amount} USDC** |
| Repository maintainer | **{maintainer_amount} USDC** |

[View escrow →](https://viewer.trustlesswork.com/{contract_id})

Thanks for contributing!"#
    )
}

pub fn split_payout_waiting_for_wallet(app_url: &str) -> String {
    format!(
        r#"### ⏳ Split payout waiting

The partial payout is ready, but a repository maintainer must connect a Stellar wallet first.

> [!IMPORTANT]
> [Open your profile →]({app_url}/profile)"#
    )
}

pub fn payout_waiting_for_wallet(reward: Decimal, username: &str, connect_url: &str) -> String {
    format!(
        r#"### ⏳ Wallet required

@{username}, connect your Stellar wallet to continue with this **{reward} USDC** bounty.

> [!IMPORTANT]
> [Connect your wallet]({connect_url}) to resume automatically.\
> Payment follows once an eligible linked PR is merged."#
    )
}

pub fn bounty_cancelled_after_refund(contract_id: &str) -> String {
    format!(
        r#"### 🛑 Bounty cancelled

A repository maintainer withdrew the escrowed funds. This bounty is closed and further work on this issue is not eligible for a payout.

[View escrow →](https://viewer.trustlesswork.com/{contract_id})"#
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
