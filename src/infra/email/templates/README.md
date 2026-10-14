# Email templates

Each file here is one notification email's HTML body. They are embedded into the
binary at compile time via `include_str!` from
`src/modules/notification/email_templates.rs`, so editing a template is just
editing the `.html` file — no Rust changes needed. Rebuild (`cargo build`) to
pick up changes.

## Files → notification kind

| File                      | Kind                      |
| ------------------------- | ------------------------- |
| `bounty_assigned.html`    | `Kind::BountyAssigned`    |
| `bounty_unassigned.html`  | `Kind::BountyUnassigned`  |
| `bounty_locked.html`      | `Kind::BountyLocked`      |
| `bounty_cancelled.html`   | `Kind::BountyCancelled`   |
| `bounty_rejected.html`    | `Kind::BountyRejected`    |
| `amount_updated.html`     | `Kind::AmountUpdated`     |
| `amount_missing.html`     | `Kind::AmountMissing`     |
| `wallet_required.html`    | `Kind::WalletRequired`    |
| `payout_released.html`    | `Kind::PayoutReleased`    |
| `payout_blocked.html`     | `Kind::PayoutBlocked`     |
| `insufficient_funds.html` | `Kind::InsufficientFunds` |
| `escrow_funded.html`      | `Kind::EscrowFunded`      |
| `escrow_refunded.html`    | `Kind::EscrowRefunded`    |

## Editing rules

- Keep the placeholder tokens intact — they are replaced at send time with
  escaped values: `TOSS_NAME_TOKEN`, `TOSS_REPO_TOKEN`, `TOSS_ISSUE_TOKEN`,
  `TOSS_AMOUNT_TOKEN`, `TOSS_ASSET_TOKEN`, `TOSS_TITLE_TOKEN`, `TOSS_BODY_TOKEN`.
- The literal `https://www.trustless-oss.xyz` is rewritten to the configured
  `app_url` at send time. Leave links using that base as-is.
- The logo `<img>` points at a hosted HTTPS URL. Do not inline/base64 it.
- Do not reintroduce `"####` sequences; these are plain HTML files now, so any
  content is fine, but keep the footer links and wording as-is.
