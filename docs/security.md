# Backend security review

Review date: 2026-11-05. Source baseline: `1d02b6996d13981fd8218bffdd35491d26285b6f` and the working tree at review time.

## Independent security fixes — 2026-11-06

At the user's request, SEC-04 and refund/withdrawal-specific security and settlement changes were excluded from this pass. SEC-04 remains critical and open. Existing refund and withdrawal implementation was not changed in this pass. The following statuses supersede older historical claims below:

| Finding | Current status and remaining work |
| --- | --- |
| SEC-01–03 | Previously fixed in source; no new production integration verification in this pass. |
| SEC-04 | **Open, critical; excluded by request.** Caller-selected refund destinations and entitlement remain unresolved. |
| SEC-05–10 | **Open.** Funding verification, deployment binding, shared accounting, concurrency, and transaction intent need further work; their shared financial paths overlap refund/withdrawal and were excluded. |
| SEC-11 | **Partially fixed.** Reward updates, reward deletion and repository deletion now require current GitHub maintainer access in addition to local membership. Payout, retry and manual-reward issue commands also recheck both. Other endpoints still need review and enforcement. |
| SEC-12 | **Partially fixed.** Payout, retry and manual-reward commands require local maintainer membership and current GitHub permission, rather than relying only on the relationship label. The refund-related reject command was excluded. |
| SEC-13 | **Partially fixed.** Compose publishes PostgreSQL and Redis only on loopback. Default PostgreSQL credentials and Redis without authentication remain unsuitable for an untrusted local host or production deployment. |
| SEC-14 | **Partially fixed.** Reward configuration and bounty reservations reject negative amounts, excess precision and values beyond the supported seven-decimal signed-64-bit amount. Pending reward changes use checked arithmetic. Zero remains valid for initially unpriced bounties. Existing data and all other accounting inputs still need a complete audit. |
| SEC-15 | **Fixed in source for direct connections.** Rate limiting uses the socket peer instead of client-controlled forwarding headers. Behind a reverse proxy, clients share the proxy's limit; a deployment-specific trusted-proxy design is needed for per-client limits. |
| SEC-16 | **Open.** Delivery IDs and event headers are not signed, and durable replay protection remains incomplete. |
| SEC-17 | **Partially fixed.** The development relay bounds its event buffer; other outbound deadlines and response sizes still need review. |
| SEC-18 | **Partially fixed.** HTTP 500 responses no longer expose internal error details. Public health endpoints and some upstream 4xx errors may still disclose operational information. |
| SEC-19 | **Fixed in source.** Configuration rejects auth bypass and the auth middleware fails closed if it is enabled. |
| SEC-20 | **Open.** Dependency advisories were not re-audited in this pass; `cargo audit` is unavailable in the environment. Historical audit results below refer to a different working tree. |
| SEC-21 | **Fixed in source for signature forgery.** The development relay verifies and forwards the original GitHub HMAC instead of signing channel messages. If Smee changes the signed body bytes, the event is rejected; a byte-preserving relay is needed. Replay and unsigned-header risks remain under SEC-16. |

These source changes do not establish production safety or resolve the open financial findings. GitHub permission checks depend on the app installation retaining the necessary repository permissions and GitHub being reachable.

### Follow-up review — 2026-11-06

- Fixed a command-loss regression in the new issue-command authorization: the shared GitHub lookup maps HTTP failures, including temporary upstream failures, to `Forbidden`. The command handler previously treated that as a successful no-op. Unconfirmed access now fails closed with a retryable error using the existing job machinery. Because the shared lookup does not distinguish these failures, a locally registered maintainer whose access was revoked will also incur retries; no command is applied without successful authorization. The shared refund authorization function was not changed.
- The relay now checks the remaining buffer capacity before appending an incoming chunk. Regression coverage verifies that an original signed raw body is preserved and that reserialized bytes fail verification.
- **Unresolved deployment concern (SEC-15):** the global peer-IP limiter uses a burst of 30 and replenishes one request per second. All clients behind the same proxy peer share that bucket, including health probes and webhook traffic. Trusted-proxy configuration needs deployment evidence; accepting arbitrary forwarding headers would reintroduce the spoofing vulnerability.
- **Unresolved relay compatibility (SEC-21):** a parsed/reserialized Smee body may no longer match the original signature. Such events are rejected. No live Smee delivery was verified; signature enforcement must not be removed to work around this.
- The review did not modify GitHub comment templates, refund/withdrawal implementation, or the previously staged changes. The open financial findings and other partial statuses above remain open; passing unit tests is not proof of production safety.

## Refund review on current branch — 2026-11-06

The refund implementation on this branch is the general maintainer-initiated flow. The restricted withdrawal implementation described in historical sections below is not present in this checkout.

This review added fresh GitHub maintainer permission and repository-ID checks for the refund endpoint, Stellar destination checksum validation, asset and milestone-data validation, and rejection of missing unsigned transactions or unsuccessful provider submission responses. It also rejects JSON number conversions that would lose a monetary amount's precision, bounds XDR parsing, stops on a failed dispute request, and avoids reprocessing terminal milestones merely because their description begins with `Refund:`. These are partial protections; they do not establish wallet ownership or validate transaction intent.

Refund preparation now fetches independent escrow data and balance concurrently. Issue notifications and GitHub comments use up to four concurrent deliveries after the refund. Cancellation updates are batched with the balance update in one database transaction, conditioned on the original contract and the bounty still being open or assigned. Duplicate issue reads and status updates were removed. The existing on-chain transaction sequence remains serial. No production timing or live transfer was measured.

Remaining findings on this branch:

- **SEC-04 — Critical, open:** a current maintainer can still choose any valid refund wallet, and funding provenance or each funder's entitlement is not verified. The server holds the signing roles and can route funds associated with active work. A valid address checksum and current GitHub permission do not prove entitlement or wallet ownership.
- **SEC-08 and SEC-09 — High, open:** refunds, funding, and payouts do not share a durable financial lock or idempotency ledger. Concurrent operations, a crash, or a timeout after a chain submission can leave the chain and database out of sync. Reconcile the transaction and escrow state before retrying an uncertain refund.
- **SEC-10 — High, open:** provider-returned XDR is size-bounded but still signed without checking the contract, operation, recipient, amount, fee, or authorization tree against the requested refund. Provider or transport compromise remains dangerous.
- **SEC-11 — High, partially mitigated:** this refund endpoint rechecks current GitHub maintainer access. Other financial endpoints still use local membership checks and require a separate review.
- **SEC-14 — Medium, partially mitigated:** JSON numeric conversion now rejects precision loss. This does not establish that every reward and accounting path validates monetary values correctly.
- **Refund settlement — High, open:** a provider `SUCCESS` response is checked, but the backend does not independently confirm final ledger state before zeroing the database balance. The cancellation query also excludes merged bounties, which need explicit settlement and reconciliation policy.

The historical statements below about operator approvals, independent signing restrictions, durable withdrawal claims, and a mainnet block do not describe this branch. Validation: 124 library tests passed; the ignored database regression was run separately against disposable PostgreSQL and verified rollback on a changed contract, preservation of a newly paid bounty, and isolation from another repository. All-target compilation passed. No production account, provider, or escrow was modified during this review.

## Current remediation status after rollback — 2026-11-06

The repository was reset to `1d02b6996d13981fd8218bffdd35491d26285b6f`. This report was subsequently recovered from Git; recovering the document did **not** restore its previously described code changes. Only SEC-01 through SEC-03 have been reapplied and checked in this pass. Later remediation notes and old test totals below are historical and must not be interpreted as protections present in this checkout.

| Finding | Current status |
| --- | --- |
| SEC-01 | Fixed in source; identity regression tests passed |
| SEC-02 | Fixed in source; GitHub authorization validation tests passed |
| SEC-03 | Fixed in source; installation permission validation tests passed |
| SEC-04 onward | Refund-related findings were partially mitigated and remain open as described above; other findings have not been revalidated in this pass |

Current behavior: a single GitHub OAuth identity supplies the numeric GitHub principal; the Supabase UUID is retained. Missing, ambiguous, conflicting, nonpositive or malformed GitHub IDs fail closed. User-editable metadata cannot select the principal or username. Auth bypass is disabled even in development.

Repository connect verifies the supplied GitHub token belongs to the authenticated user, verifies the repository ID/name and public/non-fork/admin state, and derives membership identity from that verified user. Legacy client owner fields are ignored. Failed verification or webhook installation stops before database upserts. Reconnect preserves existing rewards and installation links. A configured HTTPS `WEBHOOK_URL` is required; request Host/forwarded headers cannot choose the webhook destination.

Installation listing and sync verify current repository admin permission for the caller's numeric GitHub identity using the installation token and GitHub's collaborator-permission endpoint. All selected repositories are authorized before the first sync write. Listing returns only repositories the caller administers; explicit sync rejects an unauthorized selection. Connect/list/sync have per-user rate limits that require Redis.

## GitHub App reinstall and sync follow-up — 2026-11-06

Uninstall and installation-repository removal now detach the GitHub installation instead of deleting the repository row. The same repository UUID, escrow contract ID, observed balance, bounty records, and custom rewards remain in PostgreSQL. Local maintainer memberships are revoked on detach because the old installation no longer proves access. A later installation event or an administrator-initiated sync verifies current GitHub admin permission and rebinds the existing repository by its immutable GitHub repository ID. A delayed removal event for an old installation cannot detach a newer installation.

Installation listing now reads all pages of GitHub's paginated API, reuses one installation token and one GitHub identity lookup per request, and checks repository permissions with at most eight concurrent requests. Sync authorizes the selected set before writes and performs verified repository upserts with at most eight concurrent tasks. Installation webhooks use the same bounded checks and concurrent writes. Missing default rewards are initialized in one idempotent database statement; customized amounts are preserved. A repository administrator can resync after reinstall to regain local membership; unauthenticated or unauthorized webhook senders do not gain membership.

Previously deleted repository rows cannot be recreated with their old UUID or escrow link from a fresh GitHub installation alone. Restoring those records requires a database backup or a separately reviewed on-chain reconciliation. The change prevents future deletion in these uninstall paths; it does not claim to restore historical deletions automatically.

Verification: 117 library tests, all-target compilation, and two disposable PostgreSQL reinstall regressions passed. The database tests confirmed that an escrow contract ID, funded balance, customized reward, and repository UUID survive an uninstall/reinstall cycle; local membership is revoked while detached and restored only through the verified sync path. No production database or live GitHub installation was changed.

Validation: `cargo test --offline --lib` passed **117 tests**. New regressions cover editable metadata impersonation, absent/duplicate/conflicting GitHub identities, numeric/string subjects, token-user mismatch, repository ID/name substitution, private/forked/non-admin repositories, path injection, and mismatched or insufficient installation permissions. Provider/network errors propagate before onboarding writes. These are source-level and unit checks; no live Supabase/GitHub account or production database was mutated, and this pass does not claim full end-to-end authorization testing.

Existing memberships created before these fixes have not been audited or revoked. These fixes prevent the reviewed acquisition paths; they do not prove historic database records were legitimate. Production security depends on resolving the remaining findings as well.

## Baseline assessment and scope

**At the baseline reviewed commit, the backend contained critical authorization flaws and credible paths to unauthorized escrow withdrawals. See the current remediation statuses above.** A valid login token does not make the GitHub identity or repository permissions derived by this backend trustworthy.

This is a static source review, not evidence that an attack has already occurred. No production accounts, wallets, services, or transactions were exercised. “Confirmed” below means the unsafe behavior is visible in source; end-to-end theft was not executed. Deployment-dependent findings require the stated preconditions. This report cannot guarantee that every vulnerability has been found.

Reviewed: route composition, Supabase identity extraction, repository onboarding and permissions, wallet updates, escrow creation/funding/closing/refunds, Stellar signing, bounty reservations and payout automation, GitHub webhook ingestion and commands, queues/recovery, notifications, dashboard access, support tickets, S3 uploads, configuration, errors, and database models. The frontend, deployed Supabase settings/RLS, infrastructure permissions, Trustless Work implementation, and deployed Soroban contract code are outside this repository review.

## Priority register

| ID | Severity | Finding | Evidence classification |
| --- | --- | --- | --- |
| SEC-01 | Critical | User-editable metadata selects the authenticated GitHub identity | Fixed in source 2026-11-06; original flaw confirmed |
| SEC-02 | Critical | Repository connect grants ownership from request fields | Fixed in source 2026-11-06; original flaw confirmed |
| SEC-03 | Critical | Installation sync grants maintainer access without checking installation ownership | Fixed in source 2026-11-06; original flaw confirmed |
| SEC-04 | Critical | Refund can send escrow funds to a caller-selected wallet | Confirmed signing path; contract execution not tested |
| SEC-05 | High | Funding can credit an amount that was never verified on-chain | Confirmed |
| SEC-06 | High | Deploy/close submissions are not bound to the intended repository operation | Confirmed missing binding; helper acceptance required |
| SEC-07 | High | Chain balance reconciliation erases local reservation accounting | Confirmed |
| SEC-08 | High | Reservation/refund writes lack atomic concurrency and idempotency controls | Confirmed write pattern; concurrency behavior needs integration tests |
| SEC-09 | High | Escrow milestone mutations lack a shared contract-level lock | Confirmed missing serialization; chain impact conditional |
| SEC-10 | High | Backend blindly signs transactions returned by an external service | Confirmed; upstream/configuration compromise required |
| SEC-11 | High | Maintainer permissions persist after GitHub access changes | Confirmed missing revocation path |
| SEC-12 | High | GitHub relationship labels authorize financial commands | Confirmed policy gap; actual GitHub permissions matter |
| SEC-13 | High | Compose exposes database and Redis with unsafe access defaults | Confirmed configuration; network reachability required |
| SEC-14 | Medium | Negative rewards and unsafe monetary conversions are accepted | Confirmed |
| SEC-15 | Medium | IP rate limiting trusts forwarded headers | Deployment-dependent |
| SEC-16 | Medium | Signed webhooks can be replayed without durable event protection | Requires a captured valid signed delivery |
| SEC-17 | Medium | Outbound requests lack explicit deadlines and response limits | Confirmed configuration gap |
| SEC-18 | Medium | Internal errors and operational probes expose details | Confirmed |
| SEC-19 | High | Development auth bypass can impersonate a fixed user if exposed | Configuration-dependent; production guard exists |

Severity reflects potential impact and prerequisites, not a measured probability of exploitation. Findings can combine: SEC-02 or SEC-03 can provide the maintainer privilege needed by SEC-04 without exploiting Supabase at all.

## Findings

### SEC-01 — User-editable metadata controls authorization identity

**Current status: Fixed in the 2026-11-06 reapplication.** Authorization identity now comes only from one verified GitHub identity; editable metadata and numeric-user-ID fallbacks are removed. Regression tests cover impersonation and ambiguous identities.

**Evidence:** `src/middleware/auth.rs`, `github_id`, `github_username`, and `AuthedUser::from_request_parts`.

The bearer token is checked with Supabase `/auth/v1/user`, but `github_id()` first reads `user.user_metadata.provider_id`, then `user_metadata.sub`, before consulting a GitHub identity. A token proves who the Supabase user is; it does not turn that user's editable metadata into an authoritative GitHub identifier. The username follows the same unsafe precedence.

**Prerequisite and impact:** An ordinary authenticated Supabase account able to update its own metadata can claim another GitHub numeric ID. Downstream checks use that ID for maintainer permissions, contributor wallets, profiles, tickets, notifications, and dashboard data. This is cross-account impersonation, not JWT signature forgery. It can enable payout redirection and escrow operations. A linked GitHub identity is not required when the metadata fallback succeeds.

Supabase explicitly documents user metadata as user-editable and unsuitable for authorization: [Supabase users](https://supabase.com/docs/guides/auth/users), [Supabase metadata security guidance](https://supabase.github.io/splinter/0015_rls_references_user_metadata/).

**Fix:** Retain the immutable Supabase user UUID as the session principal. Resolve GitHub identity through a server-managed mapping established from verified OAuth identity data. Reject absent, ambiguous, or inconsistent GitHub identities. Remove metadata and numeric-user-ID fallbacks from authorization; treat usernames as display data only.

**Acceptance test:** Two isolated test accounts must remain distinct after one changes `provider_id`, `sub`, `user_name`, or `preferred_username` to values belonging to the other. Verify wallet, refund, profile, notification, and ticket isolation.

### SEC-02 — Connect endpoint lets a user grant repository ownership

**Current status: Fixed in the 2026-11-06 reapplication.** Connect verifies the GitHub token belongs to the authenticated user, verifies repository ID/name/public status/admin permission, derives membership from that user, and fails on webhook-install errors. Canonical HTTPS WEBHOOK_URL is required.

**Evidence:** `src/modules/repo/handlers.rs::connect_repo` ignores `_user`; `src/modules/repo/service.rs::connect_repo`; `src/modules/repo/repository.rs::{upsert_repo, upsert_repo_with_maintainer}`; `src/modules/github/auth.rs::install_repo_webhook`.

`POST /api/v1/repos/connect` trusts submitted repository ID/name and owner ID/username. It does not bind those fields to the authenticated caller or verify the caller's GitHub administration permission. Failed webhook installation does not stop the database upsert; some HTTP failures are merely logged. `upsert_repo_with_maintainer` writes an `owner` membership, including for an existing repository.

**Impact:** An ordinary logged-in user can request membership for their own GitHub ID on a target repository. Existing metadata, installation linkage, and reward defaults can also be overwritten. The resulting membership passes `is_maintainer`, opening refund, reward, rejection, and deletion operations.

**Fix:** Derive repository identity from GitHub, verify the authenticated user's repository administration permission with a token bound to that user, and authorize before any upsert. Never accept client-provided owner identity as authority. Separate onboarding from updates and fail closed on failed verification.

**Acceptance test:** A non-member cannot connect or modify an existing target repository using an invalid GitHub token, mismatched ID/name, or their own owner ID. No database rows should change on failure.

### SEC-03 — Installation sync grants arbitrary installation access

**Current status: Fixed in the 2026-11-06 reapplication.** Installation listing and each synchronized repository require current GitHub admin permission for the authenticated numeric GitHub identity. Sync is rate limited.

**Evidence:** `src/modules/repo/handlers.rs::{list_installation_repos, sync_installation}`, `src/modules/repo/service.rs::sync_installation`, `src/modules/github/repository.rs::upsert_installation_repo`.

The caller supplies `installation_id`. The backend uses its own GitHub App credentials to list that installation's repositories, then passes the caller's GitHub ID as `installer_github_id`. The repository function creates a maintainer membership for that installer. No check establishes that this caller installed, owns, or administers the installation or repository. Filtering out private/forked repositories does not establish authorization.

**Impact:** A user who knows a target installation ID can acquire maintainer access on its eligible repositories. Listing is also unscoped. Installation IDs must not be treated as access credentials.

**Fix:** Verify the caller's rights to the installation and each selected repository using GitHub-authoritative user permissions. Restrict membership creation to that verified identity/permission flow. Apply the same check to installation listing.

**Acceptance test:** User A cannot list privileged installation context, sync repositories, or acquire membership from user B's installation ID.

### SEC-04 — Refund destination is arbitrary and the backend controls both signing roles

**Historical pre-rollback status: Mitigated; restricted testnet retirement withdrawals implemented.** The arbitrary refund branches have been removed. Native remaining-funds withdrawals now require an operator-configured, expiring exact destination/amount approval, no outstanding local bounties, and only terminal on-chain milestones. A durable one-time claim freezes the escrow against new allocations and replay. General refunds/dispute distributions and automated mainnet signing remain blocked. A verified deposit-ownership ledger and independent signing authority remain outstanding; see the V1 follow-up below.

**Evidence:** `src/modules/escrow/handler.rs::refund`; `src/modules/escrow/trustless_work/escrow_service.rs::refund`; `src/modules/escrow/trustless_work/tx_builder.rs::create_escrow`.

Original finding (before remediation): the API checked only `is_maintainer`, then took `body.maintainer_wallet` as the destination. The service uses the platform key to dispute milestones and the resolver key to distribute funds to that address. The legacy branch rewrites milestone receivers. Remaining funds also use the supplied destination. There is no verified relationship to the original funder, recorded entitlement, wallet signature, or independently approved dispute decision.

**Impact:** Any maintainer, including one obtained through SEC-01/02/03, can request transfer of refundable escrow funds to their chosen wallet. This includes amounts attached to unfinished milestones, not just an unallocated balance. Actual execution depends on the deployed contract's roles and state, but the backend explicitly builds and signs this flow. Legitimate maintainership is also an overly broad withdrawal capability if contributors or multiple funders expect protection.

**Fix:** Define withdrawal entitlements separately from maintainership. Record verified funding provenance and refund only to authorized destinations, with explicit rules for reserved/disputed work. Require recent authentication and approval bound to contract, amount, recipient, and nonce. Separate the resolver authority from the ordinary web process; a wallet signature alone does not establish entitlement to someone else's deposit.

**Acceptance test:** A maintainer cannot redirect another funder's deposit or withdraw reserved funds contrary to the escrow policy. Test both refund branches and a multi-funder escrow.

### SEC-05 — Funding creates an optimistic, unverified credit

**Historical pre-rollback status: Fixed for unverified local credit; settlement ledger pending.** Funding requires a single-use intent bound to transaction contents, repository, network, current contract, wallet, and amount. Flat/insufficient observed balance is rejected instead of inventing credit. A durable transaction-event settlement/provenance ledger and independent ledger verification remain pending.

**Evidence:** `src/modules/escrow/handler.rs::submit_fund`; `src/modules/escrow/trustless_work/escrow_service.rs::{fund, wait_for_balance_after_fund}`.

`fund()` submits arbitrary signed XDR, ignores `_funder_wallet`, and accepts a separate client `funded_amount`. It does not verify that the transaction transferred that token/amount to this repository's contract. When repeated balance reads remain flat or lower, the function returns `previous_balance + funded_amount` and persists it. There is no verified funding-provenance ledger or transaction-hash uniqueness check.

**Impact:** A maintainer can produce a false local balance if the helper accepts a transaction unrelated to the claimed deposit, or when an actual deposit has a different amount. Retries also lack application-level credit idempotency. This can mislead users and overcommit bounties. It does not create on-chain tokens: release checks and the contract still limit actual spending.

**Fix:** Bind funding to a persisted intent and verify final ledger success, network, asset, contract, sender, and exact amount. Credit once using a unique transaction/event identifier. Keep delayed confirmation pending; never substitute a requested amount for observed settlement.

**Acceptance test:** A successful unrelated transaction, mismatched amount, replay, and flat indexer response must never increase the ledger credit.

### SEC-06 — Signed transaction submissions are not bound to their purpose

**Historical pre-rollback status: Partially fixed.** Deploy/fund/close submissions must match expiring, single-use transaction intents. Active contract replacement and duplicate binding are rejected; close requires zero observed funds and no outstanding bounties. Final receipt and contract-role verification still rely on the external provider and require independent integration validation.

**Evidence:** `src/modules/escrow/trustless_work/escrow_service.rs::{deploy, close, submit_signed_transaction}`; `src/modules/escrow/repository.rs::{update_repo_escrow_contract, clear_repo_escrow}`.

There is no saved unsigned transaction intent, expected hash, operation validation, or final state check connecting a submitted XDR to `repo_id`. `close()` clears the local contract and balance after helper success without confirming that the target contract closed. `deploy()` assigns the returned `contractId` without validating its token, roles, provenance, or prior repository binding. Existing contract references can be replaced and the schema does not make contract IDs unique.

**Impact:** A successful unrelated submission can detach a live escrow if the helper accepts it; a different deployment can attach an unintended contract. This can strand funds or point automation at the wrong escrow. Helper response semantics require integration verification.

**Fix:** Persist single-use operation intents, validate signed XDR against the intended operation, verify final contract state, and enforce an explicit escrow lifecycle. Prevent replacing an active funded contract and duplicate cross-repository bindings.

**Acceptance test:** Cross-repository XDR, a non-close transaction sent to close, unexpected roles/token, duplicate submissions, and a second deploy against a funded repository must be rejected without local mutation.

### SEC-07 — Chain totals and available balances use the same field

**Historical pre-rollback status: Fixed for reservation reconciliation.** Observed chain totals are reduced by active durable bounty reservations under a repository row lock. Financial repository reads bypass cache. The PostgreSQL regression test verifies that syncing 100 with 70 reserved leaves 30 available.

**Evidence:** `src/modules/bounty/repository.rs::create_bounty_and_reserve_balance`; `src/modules/escrow/trustless_work/escrow_service.rs::sync_balance`; `src/modules/github/handlers/helpers.rs::sync_repo_balance`.

Bounty creation subtracts the reward from `escrow_balance` as a reservation. Synchronization overwrites that same field with the full observed contract balance. Reserving a milestone does not itself transfer the corresponding funds out of the contract.

**Impact:** A synchronization can make reserved money appear available again. For example, a chain balance of 100 with 70 reserved becomes locally 30, then returns to 100 after sync. Further bounties can commit the same funds. This creates insolvency between promised rewards and available funds, even if the contract prevents spending more than its actual balance.

**Fix:** Maintain separate settled balance, active reservations, and available balance. Reconcile chain totals without discarding reservations. Derive availability transactionally from a durable ledger.

**Acceptance test:** Reserve 70 from 100, synchronize repeatedly, and verify availability remains 30 until a valid release/cancellation changes the reservation.

### SEC-08 — Balance updates are vulnerable to races and partial completion

**Historical pre-rollback status: Fixed for reviewed reservation/cancellation paths.** Reservations and reward adjustments lock the repository row. Cancellation changes status and credits the durable amount in one transaction; repeats and caller-supplied refund amounts cannot add credit. Concurrent PostgreSQL tests passed. This is not a claim of atomicity across blockchain and database boundaries.

**Evidence:** `src/modules/bounty/repository.rs::{create_bounty_and_reserve_balance, update_pending_bounty_reward, cancel_bounty_with_refund}`; `src/modules/escrow/repository.rs::refund_repo_balance`; `src/infra/db.rs::connect`.

Reservation uses a transaction, but reads a balance and writes a computed absolute value without an explicit row lock, conditional decrement, or version check. No serializable isolation is configured in this code. `balance_version` exists but is unused. Cancellation marks status and refunds separately, using the balance from a previously loaded `Repo`, without an atomic “active to cancelled” claim or unique refund record.

**Impact:** Under ordinary read-committed behavior, concurrent reservations can both pass an insufficient-funds check and overwrite each other's deductions. Concurrent/stale refunds can lose updates, and retry after partial failure can mis-account. Exact schedules need database integration tests; merely wrapping one path in a transaction does not prove safe reservation semantics.

**Fix:** Use row locking or atomic conditional balance updates with checked affected-row counts. Make status transitions, reservation changes, and ledger entries one transaction with unique operation IDs. Never refund from a stale object snapshot.

**Acceptance test:** Concurrent distinct reservations cannot exceed availability; concurrent cancellation/retry produces one refund; crash injection between status and balance changes preserves the invariant.

### SEC-09 — Shared escrow mutations are not serialized

**Historical pre-rollback status: Partially fixed.** A cross-process PostgreSQL advisory lock serializes shared-account financial mutations, including nested helpers. Payouts reload and re-evaluate under the lock. New milestones carry stable bounty markers and reject rewriting terminal/approved/disputed or differently marked entries. Ambiguous chain submissions, indexer lag, legacy milestone mappings, and database/chain crash recovery still need a durable operation ledger and fault-injection validation.

**Evidence:** `src/modules/escrow/trustless_work/escrow_service.rs::{push_milestone, refund, release_milestone}`; `src/infra/queue.rs::{enqueue_bounty_job, start_workers}`; `src/modules/bounty/service.rs::advance_inline`; `src/infra/jobs/advance.rs`.

`push_milestone()` reads the entire milestone array, appends or replaces an entry, then sends the entire array back. Jobs are organized by issue and can run concurrently for different issues on the same escrow. Refunds and inline fallback bypass a common contract mutation lock. If the chain update succeeds but saving `milestone_index` fails, a retry with no index can append another milestone.

**Impact:** Competing updates can overwrite entries or associate multiple issues with the same index; partial failure can create orphan/duplicate milestones. Stellar sequence checks may reject some competing transactions, but they do not establish application-level identity or prevent later stale updates. Wrong-recipient or duplicate-payout consequences require contract/integration testing and are not claimed as demonstrated theft.

**Fix:** Serialize mutations per contract across every worker, API, and recovery path, with fencing and durable operation IDs. Bind each milestone to a stable bounty identifier and verify identity/recipient/amount before release. Recover successful chain operations before retrying creation.

**Acceptance test:** Concurrent pushes for two issues, wallet changes during release, refund during release, and crash after chain success must preserve a unique, correct bounty-to-milestone mapping.

### SEC-10 — External transaction builder has unrestricted signing authority

**Historical pre-rollback status: Mitigated; mainnet blocked.** Automated mainnet signing and new mainnet deploy/funding preparation are blocked. HTTPS configuration, redirect rejection, bounded XDR decoding, and removal of raw transaction-result logging reduce exposure. Testnet signing still trusts the builder; strict contract/operation/recipient/fee policy and isolated resolver custody remain open.

**Evidence:** `src/infra/stellar/signer.rs::{sign_and_send_transaction, sign_xdr}`; `src/modules/escrow/trustless_work/escrow_service.rs::sign_platform_transaction`; `src/config.rs`.

The signer parses and signs returned XDR without validating operation types, source accounts, contracts, recipients, amounts, fee ceilings, or time bounds. It accepts normal, legacy, and fee-bump envelopes. Both platform and resolver secrets are available to the same backend. API/base URL configuration is not restricted to HTTPS/approved hosts; XDR parsing uses `Limits::none()`.

**Prerequisite and impact:** A compromised transaction-building service, malicious response, unsafe configured transport, or backend compromise can turn a legitimate escrow action into an arbitrary authorized transaction. The blast radius includes the signing accounts and contracts trusting them. There is no demonstrated public endpoint that directly signs arbitrary caller XDR; submit endpoints forward already-signed transactions.

**Fix:** Validate a strict transaction policy against the specific operation intent before signing, including all embedded operations/auth data, network, destination, amounts, fees, and expiry. Reject unsupported envelope types and bound parser size/depth. Use isolated signing services with separate resolver approval, limited privileges, and authenticated HTTPS endpoints.

**Acceptance test:** Mutated builder responses containing an unrelated payment, signer change, unexpected contract call, excessive fee, or extra operation must be refused before any signature is produced.

### SEC-11 — Repository permissions do not track revocation

**Historical pre-rollback status: Fixed for financial authorization.** Maintainer checks require an allowed local role AND fresh GitHub repository-admin permission resolved from numeric identity. Installation removals revoke memberships and cached tokens while retaining escrow records. Existing memberships and deployed GitHub permission configuration still require operational review.

**Evidence:** `src/modules/repo/repository.rs::is_maintainer`; `src/modules/github/webhook.rs::process_webhook_job`; `src/modules/github/repository.rs::upsert_installation_repo`.

Authorization checks only whether a local membership row exists, without inspecting its role or confirming current GitHub permissions. Installation handlers add memberships, but collaborator removal, organization membership changes, and installation suspension are not handled as permission revocations in the dispatcher.

**Impact:** A former maintainer with a valid login can retain financial privileges after GitHub access is removed. All membership roles have the same effective permission at these checks.

**Fix:** Define capabilities by role, process revocation events, periodically reconcile permissions, and require fresh authoritative permission checks for withdrawals or other high-impact actions. Fail closed when required permission freshness cannot be established.

**Acceptance test:** Remove a maintainer's GitHub access and verify refund/reward/deletion access is revoked within the documented maximum interval.

### SEC-12 — GitHub author association is treated as financial authorization

**Historical pre-rollback status: Fixed.** GitHub financial commands additionally require current repository-admin permission and local financial membership; author_association alone no longer authorizes them.

**Evidence:** `src/modules/github/handlers/helpers.rs::is_privileged_association`; `src/modules/github/handlers/issue_comment.rs::{handle_issue_comment_created, handle_payout_command}`.

`OWNER`, `MEMBER`, and `COLLABORATOR` authorize reward, rejection, retry, and payout commands. The comment handler does not resolve current repository permissions or check an application financial role for the commenter. A relationship category, especially organization membership, is not a sufficiently precise financial access policy.

**Impact:** Users whose relationship passes this test can affect bounty amounts and disposition without the repository-level authority the application may intend. A valid webhook HMAC authenticates GitHub's delivery, not the business authorization of every commenter.

**Fix:** Resolve the commenter's numeric GitHub ID to current repository permissions and an explicit command capability. Make on-chain dispute/refund actions require the same policy as API actions. Revalidate delayed commands before execution.

**Acceptance test:** Organization members and low-privilege collaborators without the required financial role cannot change payouts or reject bounties through comments.

### SEC-13 — Unsafe database/queue network defaults

**Historical pre-rollback status: Fixed local exposure; deployment pending.** Compose binds PostgreSQL and Redis ports to loopback. Production network isolation, scoped credentials, Redis ACLs, transport security, and persistence must still be verified; development credentials are not production-safe.

**Evidence:** `docker-compose.yml`; `src/infra/queue.rs`; `src/modules/github/auth.rs::cache_installation_token`.

Compose publishes PostgreSQL on host port 5435 using `postgres/postgres`, and Redis on 6379 without configured authentication. Bind addresses are not restricted to loopback. Redis contains jobs, rate-limit state, and cached GitHub installation tokens.

**Impact if reachable:** Database access permits modification of memberships, wallets, and balances. Redis access permits token theft, queue tampering, and bypass of the HTTP webhook signature boundary by injecting jobs. Host firewall/cloud security groups may prevent external access; those were not inspected. These settings may be intended only for local development.

**Fix:** Bind development ports to loopback. For deployment, keep both services private, require scoped credentials/Redis ACLs and suitable transport security, and restrict job producers/consumers. Do not reuse development passwords.

**Acceptance test:** From an untrusted network namespace, neither service is reachable; unauthorized clients cannot read cached tokens or enqueue jobs.

### SEC-14 — Monetary validation permits negative values and precision loss

**Historical pre-rollback status: Fixed for reviewed amount entry points.** Reward/reservation/funding/distribution amounts are positive, bounded, and limited to seven decimals. Money JSON uses exact decimal serialization with arbitrary_precision, not f64. Funding addition is checked. Amount boundary tests pass; existing invalid database rows still need reconciliation.

**Evidence:** `src/modules/repo/service.rs::update_rewards`; `src/modules/bounty/repository.rs::create_bounty_and_reserve_balance`; `src/modules/escrow/trustless_work/api_client.rs::decimal_json_number`; `src/modules/escrow/trustless_work/escrow_service.rs::wait_for_balance_after_fund`.

Reward validation rejects exactly zero rather than all nonpositive values. The reservation helper does not independently reject a negative reward; subtracting one increases balance if it reaches that helper. Positive amount validation also lacks a shared upper bound and seven-decimal precision policy. `decimal_json_number` converts exact decimals through `f64`, and unchecked decimal arithmetic appears in funding and payout calculations.

**Impact:** Invalid reward configuration, accounting errors if downstream paths accept it, precision mismatch with signed transfers, and possible arithmetic failures at extreme inputs. Some automation rejects nonpositive rewards, which reduces but does not repair the validation gap. This is not evidence of negative on-chain transfers succeeding.

**Fix:** Validate positive, bounded, exactly representable amounts at every trust boundary and financial repository method. Use checked arithmetic and canonical integer base units or exact decimal serialization supported by the API.

**Acceptance test:** Negative/zero values, excess fractional precision, extreme decimal values, and rounding boundaries fail consistently without balance changes or panics.

### SEC-15 — Forwarded headers can bypass IP limits

**Historical pre-rollback status: Fixed header spoofing; proxy tuning pending.** The limiter uses socket peer IP rather than untrusted forwarded headers. Added user quotas for connect/sync/uploads/ticket creation. Behind a proxy, users share the peer-IP bucket until a verified trusted-proxy design is configured.

**Evidence:** `src/middleware/rate_limit.rs::ip_limiter_config`; `src/app.rs::build_app`.

`SmartIpKeyExtractor` uses forwarded client-IP headers. This is safe only if a trusted ingress replaces untrusted headers and the backend cannot be reached directly. The application does not define a trusted-proxy boundary. The extractor's behavior is documented by [tower_governor](https://docs.rs/tower_governor/latest/tower_governor/key_extractor/struct.SmartIpKeyExtractor.html).

**Impact:** Under an unsafe proxy/direct-exposure configuration, varying headers defeats the global limit and can increase in-memory limiter cardinality. Repository onboarding, uploads, ticket creation, and several expensive bounty actions lack their own per-user quotas. Escrow's Redis per-user limit is a useful independent control and fails closed.

**Fix:** Use socket peer identity for direct traffic or a narrowly trusted proxy extractor, prevent direct origin access, and add per-user/resource quotas and concurrency limits to expensive operations.

**Acceptance test:** Forged forwarded headers cannot create new quota identities through either the ingress or direct origin path.

### SEC-16 — Webhook replay protection is incomplete

**Historical pre-rollback status: Partially fixed.** Webhook job IDs and persistent accepted markers use the HMAC-authenticated body digest. Event kind must match the body; all installation events are queued; queue/store failures fail closed. The accepted marker is written only after enqueue to avoid acknowledging lost events. Queue enqueue and marker persistence are not one atomic operation; durable Redis and business-level idempotency remain necessary.

**Evidence:** `src/modules/github/routes.rs::handle_github_webhook`; `src/infra/queue.rs::enqueue_webhook`; `src/modules/github/webhook.rs`.

HMAC verification correctly covers the body. Delivery IDs are optional and not part of the signed body; they are the basis of queue deduplication. Completed jobs are retained only for bounded time/count. Installation events execute inline without this dedupe, and unavailable-queue fallback also lacks a durable replay claim.

**Prerequisite and impact:** Someone with a captured valid body and signature can attempt redelivery with a different/absent delivery ID. This does not permit arbitrary body forgery, but stale signed events may repeat state changes or reapply obsolete membership/repository state. Existing terminal-state and live-merge checks mitigate some payout replay effects.

**Fix:** Persist authenticated event identity/body digest and processing outcome across queue and inline paths. Add business-level idempotency and current-state checks to all mutations, particularly installation updates and cancellations. Do not rely exclusively on job retention.

**Acceptance test:** Replaying the same signed event with changed/absent headers, after job pruning, and during queue outage produces no additional financial or permission mutation.

### SEC-17 — Outbound work lacks bounded execution

**Historical pre-rollback status: Partially fixed.** Shared HTTP calls have 5-second connect and 30-second total timeouts with redirects disabled. Supabase, central GitHub JSON, and Trustless Work responses are capped at 2 MiB. Public health endpoints no longer call dependencies. Dedicated outbound concurrency/circuit-breaker controls and remaining SDK-specific limits require further work.

**Evidence:** `src/state.rs::AppState::new` creates `Client::new()`; `src/modules/escrow/trustless_work/api_client.rs::tw_fetch` reads the entire response; `src/middleware/auth.rs` calls Supabase for each authenticated request; `src/routes/health.rs` performs dependency probes.

The shared client has no explicit connection/request deadlines or response size limits. External calls sit in request paths and money workers. No application-wide outbound concurrency budget is shown.

**Impact:** Slow or oversized upstream responses can tie up request resources and queue workers. Unauthenticated probes and weakly limited routes amplify dependency work. Availability failure is plausible; no load test was performed.

**Fix:** Set connection and total deadlines, bound response bodies, cap concurrent upstream work, and apply retry budgets/circuit breaking. Preserve ambiguous transaction outcomes as pending reconciliation instead of blindly resubmitting money operations.

**Acceptance test:** Slow, stalled, and oversized mock responses terminate within a bounded budget without consuming all workers or duplicating submissions.

### SEC-18 — Internal failures and operational endpoints disclose details

**Historical pre-rollback status: Fixed for reviewed public disclosure.** Internal errors return a generic message; upstream transaction-provider errors are sanitized. Detailed queue/dependency probes are unmounted and public health returns minimal liveness. Restricted log access and deployment logging policy still need verification.

**Evidence:** `src/error.rs::IntoResponse` returns `self.to_string()` even for internal/database errors; `src/modules/escrow/trustless_work/api_client.rs::tw_fetch` incorporates raw upstream response text; `src/routes/{health,queue}.rs` expose unauthenticated operational endpoints.

**Impact:** Clients can receive database/upstream diagnostics and operational state useful for reconnaissance. Certain errors could contain identifiers or sensitive provider details. This review did not establish that a specific secret is currently returned; raw error disclosure should not be equated with proven credential leakage.

**Fix:** Return stable public error codes/messages with correlation IDs; keep sanitized diagnostics in restricted logs. Expose minimal public liveness and protect detailed dependency/queue diagnostics.

**Acceptance test:** Trigger database and provider failures in mocks and verify responses contain no raw SQL, credentials, provider bodies, or internal paths.

### SEC-19 — Development bypass grants a real fixed identity

**Historical pre-rollback status: Fixed.** The authentication bypass branch is removed and startup rejects AUTH_BYPASS=true in every environment. Config no longer derives Debug, reducing accidental credential logging.

**Evidence:** `src/config.rs::from_env`; `src/middleware/auth.rs::AuthedUser::from_request_parts`.

`AUTH_BYPASS=true` returns a fixed GitHub identity without reading a bearer token. The code correctly refuses bypass unless `NODE_ENV=development`, so this is not an unconditional production bypass. However, the environment name alone does not ensure the server is private or its database and wallet credentials are disposable.

**Impact:** An exposed development instance using real data/keys grants unauthenticated callers the privileges of that fixed identity.

**Fix:** Compile bypass only into explicit development builds, use a synthetic identity and isolated fixtures, and enforce loopback/test infrastructure when enabled. Deployment validation should reject bypass and development mode for any real-funds environment.

**Acceptance test:** Production builds cannot enable bypass; development bypass cannot connect to protected production resources.

## Additional review items and conditional risks

These are gaps or questions to resolve, not demonstrated independent theft paths:

- **Wallet changes:** `src/modules/contributor/routes.rs::connect_wallet` permits an authenticated session to replace a payout destination without recent-auth verification, a signed wallet challenge, or an approval/cooldown flow. Stellar account/trustline validation exists. Selecting a payout address without proof of ownership is not inherently an authentication bypass, but it amplifies SEC-01 or session theft. Non-Stellar chains bypass account validation and are stored although milestone creation only supports Stellar. Reject unsupported chains and define protected destination-change rules.
- **Attachment references and upload content:** `src/modules/ticket/service.rs` validates `attachment_key` length but not ownership; `src/modules/ticket/repository.rs::create` accepts a bounty reference without a relationship check. `src/modules/s3/handler.rs` accepts arbitrary nonempty MIME types and has no per-user storage quota. Keys are randomized and scoped by user, folder characters are checked, upload size is bounded, and content length is supplied when presigning. A cross-user download or stored-XSS exploit is not established because attachment serving/rendering is outside the reviewed code. Enforce object ownership, isolated attachment serving, safe content disposition, and a content policy.
- **Webhook URL from headers:** `src/modules/repo/handlers.rs::resolve_webhook_url` trusts `Host`/`x-forwarded-proto` when `WEBHOOK_URL` is unset. A caller with a GitHub token that can install hooks could cause delivery to an unintended URL. Set and validate a canonical HTTPS webhook URL. This is not proof that arbitrary clients can change hooks without GitHub permission.
- **Installation-wide deletion:** `src/modules/repo/service.rs::delete_repo` decides whether to uninstall the entire GitHub App from a count of locally stored repositories. Unsynced/private/forked repositories are not represented in that count. It also trusts a local balance for deletion; installation-removal webhooks remove local repositories without escrow settlement checks. Preserve funded escrow records in an archival/disconnected state, verify chain settlement, and separate repository deletion from installation-wide administration.
- **Direct database API/RLS exposure:** The migration does not establish RLS policies or revoke public API grants. This alone is not a vulnerability in a private backend-only database. If these tables are exposed through Supabase/PostgREST, inspect effective roles, grants, and RLS separately; direct access could bypass every Rust authorization check. Do not assume Supabase Auth implies secure database access.
- **Secrets and transport:** Config contains platform/resolver private keys and API credentials as ordinary strings. This review did not inspect private `.env` values or exhaustively scan Git history, and does not assert that production secrets are committed. A fixed test secret is present in the signer's unit test; ensure it is never funded or reused. Validate exact network names instead of mapping every non-`mainnet` string to testnet; enforce consistent token, provider endpoint, and signing network.
- **Public data policy:** Repository issues, the public leaderboard, and authenticated repository details/rewards have deliberately broad visibility. Confirm only public repository data can enter these views, especially because legacy connect does not validate public/private status. UUID secrecy is not access control.
- **Dependency advisories:** The follow-up installed cargo-audit under `/tmp`, audited the lockfile, updated compatible dependencies, disabled legacy AWS TLS features, and removed the Swagger build-time ZIP dependency. See SEC-20 below for the remaining advisory and warnings. A clean dependency audit is not claimed.

## Controls already present

- Supabase is contacted to verify bearer tokens; ordinary missing/invalid tokens fail closed when bypass is disabled. The defect is the subsequent identity mapping.
- GitHub webhook signatures use HMAC-SHA256 and `verify_slice` before JSON dispatch; malformed signatures are rejected.
- Escrow handlers have maintainer checks and shared per-user rate limiting. Those checks become ineffective when memberships or identity are forged.
- Bounty wallet connection checks the assigned contributor. Payout automation checks the live linked PR merge, author, issue relationship, and receiver/amount state. Full release checks dispute/release state and live balance.
- Queue jobs have deduplication/retry controls and payout workers re-evaluate current rules. These do not replace a contract-wide lock or a durable money ledger.
- Ticket reads/updates/resolution are profile-scoped, and notification marking checks ownership. SEC-01 can still undermine the principal used by those checks.
- Stellar account/trustline validation, decimal types in application logic, explicit CORS origins, and randomized upload keys provide useful safeguards. They do not repair the critical authorization flaws.

## Remediation order

1. Fix SEC-01, SEC-02, and SEC-03 together. Audit existing memberships and repository bindings against GitHub before trusting them; patching new requests does not undo previously granted access.
2. Restrict refund entitlement and destinations (SEC-04), separate resolver authority, and validate operation intents before every signature/submission (SEC-05/06/10).
3. Implement a durable settlement/reservation ledger, atomic status transitions, and contract-wide serialized mutation/reconciliation (SEC-07/08/09). Reconcile current balances, reservations, and milestone identities before re-enabling automated transfers.
4. Correct permission revocation and comment-command policy (SEC-11/12), and verify database/Redis isolation (SEC-13).
5. Complete monetary validation, replay defenses, proxy/rate-limit policy, outbound limits, sanitized errors, wallet-change protection, and dependency/deployment checks.

## Follow-up findings from rechecking the fixes

### SEC-20 — Remaining dependency advisory and maintenance warnings

**Severity: High potential impact; current runtime reachability not established. Status: Partially fixed.** The initial RustSec scan found 29 vulnerability entries. Compatible lockfile updates and removal of legacy AWS TLS and Swagger ZIP extraction dependencies reduced that to **one remaining vulnerability entry**, [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html), for `rsa 0.10.0-rc.20`. The advisory database reports no patched version. `cargo tree -i rsa` printed no active reverse dependency for the current Linux configuration, so the lockfile hit is not proof of a reachable backend RSA operation. The existing GitHub JWT code uses jsonwebtoken's RSA implementation, not a newly added direct dependency on this crate.

An unsoundness warning remains for `lru 0.16.4` ([RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)); its reverse dependency also did not appear in the current active tree. Unmaintained-package warnings remain for `json`, `paste`, `proc-macro-error`, and `rustls-pemfile`. Do not suppress these globally: inspect the resolved dependency graph for each deployment target/feature set and prevent optional features from activating vulnerable implementations. The final scan used the fetched RustSec database with `--no-fetch --no-yanked`; it is not a final yanked-package check.

### Recheck corrections applied

- A separate refund route existed in GitHub dispute commands; it is now blocked at the dispute-distribution helper as well as the escrow refund service.
- A replay marker written before enqueue could acknowledge an event that was never queued. The marker now follows successful enqueue, and body-digest job IDs cover concurrent enqueue attempts. The remaining two-step crash window is recorded under SEC-16.
- The patched BullMQ version rejected legacy colon-containing custom IDs. New IDs now use a stable separator-free digest, and financial enqueue/watchdog paths also recognize legacy IDs. This permits retaining patched Tokio instead of pinning the old queue library that required an affected Tokio version. Stop old workers before deploying; do not mix old and new financial workers.
- An intent prepared for an earlier escrow could otherwise be submitted after repository contract changes. Intents now include the contract and network and verify the current binding before submission.
- Financial reads from Redis could otherwise retain old balances or contract references after failed invalidation. Repository financial reads now go directly to PostgreSQL.
- Payout evaluation before acquiring the financial lock could become stale. Release now reloads and re-evaluates within the lock.
- Syncing a chain total after reservations could restore spent availability. Reconciliation now subtracts active reservations and is covered by a real PostgreSQL test.
- Uninstalling a GitHub App could erase accounting records. Uninstall handling now revokes membership while retaining escrow/bounty records; deleting one repository no longer automatically uninstalls the entire account-wide App.

### Remaining release blockers and operational checks

1. Implement independently verified transaction receipts and a durable financial operation/funding provenance ledger. Resolve ambiguous submissions before retrying; do not infer successful settlement from HTTP success alone.
2. Implement and test the strict signing policy, wallet/destination approval, and independent dispute-resolution authority before restoring mainnet or refund/distribution capabilities.
3. Audit existing memberships, negative/invalid amounts, contract bindings, reservations, and legacy milestone indices against GitHub and the chain. Runtime checks prevent new authorization bypasses but do not repair historic data automatically.
4. Verify Supabase grants/RLS, signing-key custody, GitHub App permissions, proxy behavior, Redis persistence/ACLs, database networking, S3 serving policies, and target-specific dependency reachability in the actual deployment.
5. Add end-to-end mocked GitHub/Supabase authorization tests and contract/testnet fault-injection tests for partial success, retries, RPC lag, database disconnection while holding advisory locks, and process termination. The global financial lock intentionally limits concurrency and is not a replacement for a settlement ledger.

## Validation status and follow-up

The follow-up used an isolated PostgreSQL instance and Redis container, not the application's data stores. No live escrow transfer or production account mutation was attempted. The existing `docs/security.md` is the only security report; the original findings remain for traceability.

Validation results are recorded below after final checks. Regression tests in `tests/security_regressions.rs` are explicitly ignored by default and require disposable `SECURITY_TEST_DATABASE_URL` / `SECURITY_TEST_REDIS_URL` settings; they must not silently pass against a missing service.

Final verification after the queue compatibility correction:

- `cargo test --offline --all-targets` against the disposable Redis: 118 library tests and 13 queue integration tests passed; binary targets had no tests. The three opt-in security tests were ignored in this default invocation, then run explicitly below.
- `cargo test --offline --test security_regressions -- --ignored --test-threads=1` against disposable PostgreSQL and Redis: all 3 passed. These cover concurrent reservation/one-time cancellation, preservation of reservations during reconciliation, cross-connection/reentrant financial locking, and transaction intent repository/action/amount/replay isolation.
- Unit tests additionally cover editable-metadata impersonation, missing/ambiguous GitHub identities, signed-body event classification, monetary boundaries, bounded/malformed XDR, transaction-content changes versus signature additions, and separator-free queue IDs.
- `cargo check --offline --all-targets`, formatting checks, and whitespace checks passed during the final verification sequence.
- Final RustSec scan: one unresolved `rsa` vulnerability entry, one `lru` unsoundness warning, and four unmaintained-package warnings. Tokio was updated to a patched version. Audit exit status remains nonzero because the unresolved advisory is not suppressed.

These results do not exercise a production Supabase/GitHub account, live contract, mainnet transaction, or deployment network policy. No production readiness claim is made. Test services were disposable and separate from application storage.

## Runtime follow-up from the development logs

### SEC-21 — Development relay signed unauthenticated public-channel input

**Severity: High. Status: Fixed at the proxy authentication boundary.** `src/dev/webhook_proxy.rs` previously discarded the incoming GitHub signature and generated a fresh valid HMAC for whatever JSON arrived over the Smee channel. Anyone able to publish to that channel could therefore pass the backend webhook authentication boundary. Development mode and TLS to Smee do not authenticate the publisher.

The proxy now verifies the original HMAC-SHA256 against the exact bytes it forwards, with constant-time MAC verification, and never creates a replacement signature in production code. It accepts an exact string body or string `rawBody`; an object body is accepted only if its serialization matches the original signature. Missing, forged, or mismatched signatures fail closed. Tests cover injection, tampering, valid signatures, and preservation of signed whitespace.

**Compatibility requirement:** A relay that parses and reserializes GitHub JSON without retaining the original signed bytes may no longer work. Use forwarding that preserves the raw body and original signature. The application must not restore the old re-signing bypass to make such a relay work. No real GitHub/Smee delivery was exercised during this follow-up.

### Runtime corrections

- Smee SSE connections use a dedicated client with a connection deadline and a 90-second read timeout, but no 30-second whole-stream timeout. Ordinary HTTP API deadlines remain in place. The partial-event buffer is capped at 4 MiB. Real network interruptions can still trigger reconnection.
- Financial lock contention uses nonblocking PostgreSQL advisory-lock attempts with bounded retry outside the transaction/connection, rather than holding pool connections while waiting on a five-second SQL lock timeout. Contention still fails closed after 60 seconds. A real PostgreSQL test holds the lock for six seconds and confirms that a contender succeeds without the former five-second timeout.
- GitHub HTTP 410 is represented explicitly and resolves the funding-recovery task for a permanently deleted issue. HTTP 404, authorization errors, and transient upstream errors are not treated as proof of deletion. No historic records in the application database were manually changed.
- Recovered webhook jobs are not marked dead while BullMQ still has transient retries available. This prevents a retry from silently skipping a task that the previous attempt prematurely marked dead. Exhausted/permanent failures remain durable, and recovery queue IDs derive from the stable task UUID instead of recursively hashing prior queue IDs.
- Funding waits are logged as waits retained for durable recovery, rather than as permanent application errors. The worker still stops immediate retries and leaves the durable funding task pending. An available balance of zero, a missing payout wallet, or a missing qualifying merged PR is not bypassed.

Follow-up verification: **122 library tests passed**, plus the targeted **six-second PostgreSQL contention regression passed**. Formatting and whitespace checks passed. No production database, wallet, or real GitHub account was used for these tests.

Refund UI note (updated 2026-11-06): `RepoDetails::new` defaults `canRefundEscrow` to false. Repository detail handling now enables it only for a maintainer with a valid operator approval, no outstanding local bounties or prior withdrawal, and eligible on-chain escrow state. The refund endpoint repeats these checks. Mainnet and general active-bounty refunds remain disabled.

### Funded escrow milestone update rejection (2026-11-05)

**Status: Local defects fixed; deployed escrow verification outstanding.** The provider's `EscrowPropertiesMismatch` error is consistent with two backend defects: treating `receiverMemo` as indexer metadata and replacing an existing milestone during a repeated push. The public multi-release contract requires the memo and all existing milestone fields to remain unchanged when funded ([provider validator, pinned source](https://github.com/Trustless-Work/trustlesswork-smart-contract-stellar/blob/d5cfc2ee0341141ab0a348388771afe71c8c28d2/contracts/escrow/src/core/validators/escrow.rs)). The log does not identify the exact differing property or deployed contract version.

- The shared update-payload helper now preserves `receiverMemo`, including nonzero values, and removes only the previously identified indexer/envelope metadata. Both escrow-service and GitHub-handler callers use it.
- A repeated milestone push with matching bounty description, recipient, and decimal amount repairs the database index without rewriting the on-chain entry or clearing evidence/status. Changed terms fail closed for manual reconciliation; existing approved/disputed/settled guards remain active.
- Provider client rejections are retained as dead durable tasks immediately, so reconciliation does not repeatedly submit the same rejected operation. HTTP 408/429 remain transient. Funding waits retain their existing recovery behavior. Dead means intervention required, not successfully paid or discarded.
- Deploy the fix before retrying. An already dead task requires inspection and deliberate reactivation after its payload/contract mismatch is resolved; this change does not bulk reset historic failures or mutate production funds. A pending task can retry through existing reconciliation.

Regression coverage checks nonzero memo preservation, equivalent decimal amounts, changed payment terms, progress preservation, permanent rejection classification, and retryable provider throttling/timeouts. No live escrow update, signing operation, or production database mutation was performed.

Validation for this correction: `cargo test --lib --offline` **125 passed**; `cargo check --offline --all-targets`, `cargo fmt --check`, and `git diff --check` passed. Database-backed integration tests were not rerun for this correction.


## Trustless Work V1 withdrawal follow-up — 2026-11-06

The documented native [`POST /escrow/multi-release/withdraw-remaining-funds`](https://docs.trustlesswork.com/trustless-work/api-rest/deploy-1/withdraw-remaining-funds) requires the dispute resolver and all milestones released or dispute-resolved. It returns unsigned XDR. It does **not** establish a depositor's ownership of the remaining funds. V1 dispute resolution can distribute milestone funds, but it is not authorization to automatically cancel contributors' work. The previous milestone-rewrite/approve/release fallback was removed, not re-enabled.

Implemented policy: retire a **settled testnet escrow**, withdrawing its entire approved remaining balance to one operator-approved destination. `POST /api/v1/escrow/refund` retains its existing request/response shape. `maintainerWallet` must equal the approved destination; `cancelledIssues` is always zero. No contributor bounty is cancelled. Initial/bootstrap milestones also need to be terminal; the backend does not silently resolve them to make withdrawal pass.

### Operator approval and frontend behavior

`ESCROW_WITHDRAWAL_APPROVALS` defaults to `[]`. It is a JSON array configured on the backend, never supplied by an API caller. Example structure (replace all placeholders and set `expiresAt` to a future Unix timestamp):

```json
[{"repoId":"<repository UUID>","contractId":"<escrow contract>","destination":"<verified Stellar G address>","amount":"10.00","expiresAt":0}]
```

Before configuring an approval, the operator must verify the funders' entitlement/consent and destination using independent records, and confirm there are no outstanding claims. **This is manual authorization, not an implemented provenance ledger or cryptographic second-party approval.** Do not approve an entire multi-funder pool for one person without establishing entitlement. Approval is bound to the repository and contract, expires, and must exactly equal the live balance. Asset must equal configured `TOKEN_ADDRESS`; the destination must have the required USDC payout account/trustline. The configured resolver must match the on-chain role. The existing automated mainnet prohibition remains enforced.

The existing frontend can consume `canRefundEscrow`. It is true only when the above preflight succeeds for a maintainer; a changed balance/state can still reject a later request. The frontend must submit the approved destination in `maintainerWallet`. No frontend source was modified.

### Durable withdrawal and failure handling

A transaction locks the repository row, rejects **every** bounty status other than `paid`/`cancelled`, inserts an `escrow-withdrawal` record in the existing `failed_tasks` table, and sets available balance to zero before signing. Concurrent bounty reservations use that same row lock. The record's payload binds the contract/destination/amount/expiry; its initial `dead` status deliberately excludes automatic recovery. Only the native withdrawal endpoint is used, with the resolver key. Missing XDR is an error.

Submission results are saved when available; zero observed chain balance is required before marking the record `resolved` and reporting success. The record remains present permanently even after success. Balance reconciliation holds the retired contract's available balance at zero; new funding preparation/submission is rejected. Duplicate requests cannot consume new deposits. External direct deposits cannot be prevented by this backend and require manual recovery.

Any provider error, timeout, process crash, or stale balance observation after the claim leaves the escrow frozen. Inspect the durable record and authoritative transaction/contract state; **do not delete the record or automatically replay the withdrawal**. There is deliberately no automatic unfreeze endpoint. Retire this contract and use a separately deployed escrow for future work after reconciliation. Operator approval and the resolver secret still reside in the backend trust boundary; compromise of that boundary is not addressed by this restricted feature.

Validation: **127 library tests passed**, all-target compilation passed. Disposable PostgreSQL tests exercise concurrent one-time claims, withdrawal versus bounty reservation, denial of post-claim reservations, and prevention of stale balance resurrection. No real wallet or live escrow transfer was exercised. This does not mark SEC-04's provenance-ledger/independent-signing requirements complete.

Final V1 follow-up checks: all **5 PostgreSQL security regressions passed**, including the two new withdrawal tests; the Redis-only intent test was excluded from this run. Formatting and whitespace checks passed. The disposable PostgreSQL instance was stopped after testing.

### Funding diagnostic follow-up — 2026-11-06

A reported V1 insufficient-funds rejection was investigated with a read-only public Horizon account lookup. The supplied wallet had sufficient configured testnet USDC for the reported amount of 1 at lookup time; this does not prove the signer, escrow token, or balance at the earlier failed request. The escrow contract and actual frontend request remain needed to establish the cause. Funding preparation now logs public repo/contract/signer identifiers, amount, and configured network. An exact allowlisted provider insufficient-funds message produces actionable client guidance; arbitrary provider response text remains private and the permanent-rejection classification is preserved. No balance checks were bypassed and no funding transaction was submitted. Validation: 128 library tests passed and whitespace checks passed.
