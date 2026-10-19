-- Stellar tx hash of the payout release, stored on the bounty when it is paid.
-- Nullable: existing bounties stay NULL (their on-chain hash was not captured).
ALTER TABLE "bounties" ADD COLUMN IF NOT EXISTS "payout_tx_hash" TEXT;
