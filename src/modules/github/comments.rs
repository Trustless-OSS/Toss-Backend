use rust_decimal::Decimal;

// ─────────────────────────────────────────────────────────────
// Static messages
// ─────────────────────────────────────────────────────────────

pub const HELP: &str = concat!(
    "### 🤖 TOSS Bot\n\n",
    "Use these commands in issue and PR comments.\n\n",
    "#### 🛠️ Maintainers\n\n",
    "| Command | What it does |\n",
    "| :--- | :--- |\n",
    "| `@trustless-oss <amount>` | 💰 Set a manual bounty in USDC |\n",
    "| `@trustless-oss pay <percentage>` | ✂️ Split the bounty when the PR is merged |\n",
    "| `@trustless-oss reject` | 🛑 Reject the work and refund the escrow |\n",
    "| `@trustless-oss retry` | 🔄 Force a re-check (rarely needed) |\n\n",
    "#### 👩‍💻 Contributors\n\n",
    "| Command | What it does |\n",
    "| :--- | :--- |\n",
    "| `@trustless-oss wallet` | 🔑 Connect or update your wallet |\n\n",
    "#### 💡 General\n\n",
    "| Command | What it does |\n",
    "| :--- | :--- |\n",
    "| `@trustless-oss help` | 📖 Show this command list |\n\n",
    "---\n",
    "_Payouts continue automatically once their conditions are met._\n",
);

pub const BOUNTY_UPDATE_FAILED: &str = concat!(
    "### ⚠️ Bounty Not Updated\n\n",
    "> [!WARNING]\n",
    "> The escrow balance is too low to cover this bounty.\n\n",
    "_Top up the escrow, then try again._\n",
);

pub const INVALID_SPLIT: &str = concat!(
    "### ⚠️ Invalid Split\n\n",
    "> [!WARNING]\n",
    "> The percentage must be between **1** and **99**.\n\n",
    "_Example: `@trustless-oss pay 70`_\n",
);

pub const MANUAL_AMOUNT_NEEDED: &str = concat!(
    "### ✍️ Amount Needed\n\n",
    "> [!NOTE]\n",
    "> This is a manual bounty, so it needs an amount before funds can be locked.\n\n",
    "**Set it by commenting:**\n\n",
    "```\n",
    "@trustless-oss <amount> USDC\n",
    "```\n\n",
    "✅ You can assign a contributor now.\n",
    "⏳ Escrow locking starts once the amount is set.\n",
);

pub const RETRYING_BOUNTY: &str = concat!(
    "### 🔄 Re-checking Bounty\n\n",
    "> [!NOTE]\n",
    "> Running a fresh check now.\n\n",
    "_You normally don't need this. Payouts continue on their own once their conditions are met._\n",
);

pub const UNPRICED_BOUNTY_CREATED: &str = concat!(
    "### 💰 Bounty Created\n\n",
    "> [!IMPORTANT]\n",
    "> No reward is set yet. Add one to activate this bounty.\n\n",
    "**Set the reward by either:**\n\n",
    "1. 🏷️ Adding a label: `low`, `medium`, `high`, or another listed level\n",
    "2. 💬 Commenting: `@trustless-oss <amount> USDC`\n",
);

// ─────────────────────────────────────────────────────────────
// Wallet
// ─────────────────────────────────────────────────────────────

pub fn wallet_update(login: &str, connect_url: &str) -> String {
    format!(
        "### 🔑 Update Your Wallet\n\n\
         👋 Hey @{login}! You can change your wallet address here:\n\n\
         [**Update Wallet →**]({connect_url})\n"
    )
}

pub fn maintainer_wallet_required(login: &str, app_url: &str) -> String {
    format!(
        "### 🔑 Wallet Required\n\n\
         > [!IMPORTANT]\n\
         > @{login}, connect your Stellar wallet before using this command.\n\n\
         [**Connect Wallet →**]({app_url}/connect)\n"
    )
}

pub fn contributor_wallet_missing(username: &str) -> String {
    format!(
        "### 🔑 Contributor Wallet Missing\n\n\
         > [!WARNING]\n\
         > @{username} must connect a Stellar wallet before a split can be configured.\n\n\
         _Ask them to comment `@trustless-oss wallet` to get started._\n"
    )
}

// ─────────────────────────────────────────────────────────────
// Bounty created / updated
// ─────────────────────────────────────────────────────────────

pub fn manual_bounty_created(reward: Decimal, contract_id: &str, creator: &str) -> String {
    format!(
        "### 🎯 Bounty Created\n\n\
         @{creator} created a manual bounty.\n\n\
         | Detail | Value |\n\
         | :--- | :--- |\n\
         | 💰 **Reward** | **{reward} USDC** |\n\
         | 📊 **Level** | `manual` |\n\
         | 🔗 **Escrow** | [View on-chain →](https://viewer.trustlesswork.com/{contract_id}) |\n\n\
         ---\n\
         _Next step: assign a contributor to lock the funds._\n"
    )
}

pub fn labeled_bounty_created(reward: Decimal, difficulty: &str, contract_id: &str) -> String {
    format!(
        "### 💰 Bounty Created\n\n\
         | Detail | Value |\n\
         | :--- | :--- |\n\
         | 💰 **Reward** | **{reward} USDC** |\n\
         | 📊 **Level** | `{difficulty}` |\n\
         | 🔗 **Escrow** | [View on-chain →](https://viewer.trustlesswork.com/{contract_id}) |\n\n\
         ---\n\
         _You can change the amount anytime before assignment. Assigning a contributor locks the funds._\n"
    )
}

pub fn manual_bounty_updated(amount: Decimal) -> String {
    format!(
        "###  Bounty Updated\n\n\
         The reward is now **{amount} USDC**.\n"
    )
}

pub fn labeled_bounty_updated(amount: Decimal, difficulty: &str) -> String {
    format!(
        "###  Bounty Updated\n\n\
         | Reward | Level |\n\
         | :--- | :--- |\n\
         | 💰 **{amount} USDC** | `{difficulty}` |\n"
    )
}

// ─────────────────────────────────────────────────────────────
// Balance problems
// ─────────────────────────────────────────────────────────────

pub fn manual_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    format!(
        "### ⚠️ Insufficient Escrow Balance\n\n\
         > [!WARNING]\n\
         > The escrow doesn't have enough funds for this bounty.\n\n\
         | | Amount |\n\
         | :--- | :--- |\n\
         | 💼 **Available** | {balance} USDC |\n\
         | 💰 **Required** | {reward} USDC |\n\n\
         [**Top Up Escrow →**]({app_url}/dashboard)\n"
    )
}

pub fn labeled_insufficient_balance(balance: Decimal, reward: Decimal, app_url: &str) -> String {
    format!(
        "### ⚠️ Insufficient Escrow Balance\n\n\
         > [!WARNING]\n\
         > The escrow balance is too low for this bounty.\n\n\
         | | Amount |\n\
         | :--- | :--- |\n\
         | 💼 **Available** | {balance} USDC |\n\
         | 💰 **Required** | {reward} USDC |\n\n\
         [**Top Up Escrow →**]({app_url}/dashboard)\n"
    )
}

// ─────────────────────────────────────────────────────────────
// Assignment
// ─────────────────────────────────────────────────────────────

pub fn contributor_assigned_without_amount(login: &str) -> String {
    format!(
        "### ⏳ Assigned, Waiting for Amount\n\n\
         > [!NOTE]\n\
         > @{login} is assigned, but this bounty has no amount yet.\n\n\
         **Maintainers can set one by either:**\n\n\
         1. 🏷️ Adding a reward level label\n\
         2. 💬 Commenting `@trustless-oss <amount> USDC`\n\n\
         _Escrow locking starts automatically afterward._\n"
    )
}

pub fn contributor_unassigned(reward: Decimal) -> String {
    format!(
        "### Contributor Unassigned\n\n\
         The milestone has been closed.\n\n\
         💰 The **{reward} USDC** bounty is still **open** for the next assignee.\n"
    )
}

// ─────────────────────────────────────────────────────────────
// Payout
// ─────────────────────────────────────────────────────────────

pub fn payout_intent_saved(
    percentage: i32,
    contributor_amount: Decimal,
    maintainer_amount: Decimal,
    contributor_username: &str,
) -> String {
    format!(
        "### 📋 Payout Split Saved ({percentage}%)\n\n\
         When this PR is merged, the bounty will be split:\n\n\
         | Recipient | Amount |\n\
         | :--- | :--- |\n\
         | 👩‍💻 @{contributor_username} | **{contributor_amount} USDC** |\n\
         | 🛠️ Maintainer | **{maintainer_amount} USDC** |\n\n\
         ---\n\
         _You can change this anytime before merging with `@trustless-oss pay <percentage>`._\n"
    )
}

pub fn pr_author_mismatch(issue_number: i32) -> String {
    format!(
        "### ⚠️ PR Author Mismatch\n\n\
         > [!WARNING]\n\
         > The author of this PR is not the contributor assigned to issue #{issue_number}.\n"
    )
}

pub fn payout_aborted_for_pr_author(issue_number: i32) -> String {
    format!(
        "### 🚫 Payout Aborted\n\n\
         > [!CAUTION]\n\
         > The author of this PR is not the contributor assigned to issue #{issue_number}, so no payout was made.\n"
    )
}

// ─────────────────────────────────────────────────────────────
// Cancelled / rejected
// ─────────────────────────────────────────────────────────────

pub fn bounty_cancelled(reward: Decimal) -> String {
    format!(
        "### 🛑 Bounty Cancelled\n\n\
         > [!CAUTION]\n\
         > A maintainer closed this issue, so the bounty is no longer active.\n\n\
         | Detail | Status |\n\
         | :--- | :--- |\n\
         | 📌 **Status** | ❌ Cancelled |\n\
         | 💰 **Reward** | **{reward} USDC** |\n\
         | ↩️ **Funds** | Returned to the pool |\n\n\
         > [!TIP]\n\
         > Don't be discouraged! 🚀 Browse other open bounties and try another one. Every contribution helps the community grow.\n"
    )
}

pub fn bounty_rejected(reward: Decimal, contract_id: &str) -> String {
    format!(
        "### 🛑 Bounty Rejected\n\n\
         > [!CAUTION]\n\
         > The maintainer rejected the work.\n\n\
         | Detail | Status |\n\
         | :--- | :--- |\n\
         | 💰 **Reward** | **{reward} USDC** |\n\
         | ↩️ **Funds** | Returned to the maintainer's wallet |\n\
         | 🔗 **Escrow** | [View on-chain →](https://viewer.trustlesswork.com/{contract_id}) |\n"
    )
}
