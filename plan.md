# Private contributor notifications and onboarding plan

## Goal

Keep sensitive bounty information out of public GitHub issue comments while making sure every contributor can receive the information they need through the TOSS app and email.

## Contributor onboarding

1. When GitHub assigns a contributor, look up their GitHub profile in TOSS.
2. If the contributor is not known to TOSS, create or update a lightweight profile from the GitHub identity and mark them as `onboarding_required`.
3. Send a private notification only when a usable email address exists. Otherwise, keep the in-app notification pending until the contributor logs in.
4. Include a signed or authenticated TOSS login/connect link containing the repository and issue context. Do not put wallet addresses, balances, or private payout details in the GitHub issue comment.
5. After the contributor logs in, associate the authenticated account with the GitHub profile and collect or confirm their email address.
6. If no wallet is connected, show the wallet-connect flow immediately after login and notify the contributor that payout cannot start until a wallet is connected.
7. Once the wallet is connected, resume the parked bounty job automatically and notify the contributor that the bounty is ready.

## Notification and email service

The repository already has a `notifications` table and a Resend client. Add a shared notification service instead of sending messages directly from webhook handlers.

Each notification should contain:

- recipient profile ID
- event kind
- title and private body
- related bounty or repository ID
- read state
- deduplication key and delivery status

Use the existing notification table for the in-app inbox. Add only the migration fields needed for deduplication and email delivery tracking.

Add a Resend email service with configurable sender address/name. Email delivery must be queued and retried so a Resend failure does not fail a GitHub webhook.

## Private event types

Create in-app and email notifications for:

- first-time contributor onboarding/login required
- wallet connection required
- missing bounty amount
- insufficient escrow balance
- bounty locked
- payout released
- payout failed or blocked
- bounty cancelled or rejected

Do not include private balances, wallet details, or payout breakdowns in public GitHub comments.

## Public GitHub comment policy

Remove routine status comments from issue handlers. Keep public comments only for information that GitHub participants must see, such as command help or a final public transaction link if explicitly desired.

All webhook handlers should call the notification service for private events and use structured logs for routine internal state changes.

## API and frontend support

Add authenticated endpoints to:

- list a contributor's notifications
- list unread notifications
- mark one notification as read
- mark all notifications as read
- request or complete the contributor login/connect flow

The connect link must validate the GitHub profile and issue context; a user must not be able to connect a wallet to another contributor's bounty.

## Bounty lifecycle integration

1. `issues.assigned` resolves the contributor profile.
2. New contributors receive an onboarding notification and a login link.
3. The bounty job parks while login, wallet, or amount setup is incomplete.
4. Login and wallet events enqueue a resume job for that bounty.
5. Amount, lock, payout, failure, and cancellation events create private notifications and email jobs.
6. Notification delivery is idempotent across repeated GitHub webhooks.

## Safety and testing

- Never log email addresses, wallet secrets, or authentication tokens.
- Do not send email when the profile has no verified email; retain the in-app notification.
- Use deduplication keys so webhook redelivery cannot spam a contributor.
- Test first-time onboarding, missing email, login completion, wallet completion, email retry, duplicate webhook delivery, and private-comment suppression.
- Run database migration, unit, integration, and queue tests before deployment.

## Implementation order

1. Add notification repository and service around the existing table.
2. Add notification delivery fields and migration.
3. Add Resend templates, sender configuration, and queued email jobs.
4. Add authenticated notification and onboarding routes.
5. Integrate assignment, wallet, bounty, and payout handlers.
6. Remove private status comments and add deduplication.
7. Add frontend inbox/login/connect screens and run the full test suite.
