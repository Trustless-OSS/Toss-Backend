-- restore repo reward columns
ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS reward_low numeric default 0;
ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS reward_medium numeric default 0;
ALTER TABLE repos
  ADD COLUMN IF NOT EXISTS reward_high numeric default 0;
