-- Convert skills from a text[] array to a comma-separated TEXT column.
-- Guarded so it is a no-op if skills is already TEXT (e.g. a manually patched DB).
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'profiles'
          AND column_name = 'skills'
          AND data_type = 'ARRAY'
    ) THEN
        ALTER TABLE "profiles"
            ALTER COLUMN "skills" TYPE TEXT USING array_to_string("skills", ', ');
    END IF;
END $$;

ALTER TABLE "profiles" ADD COLUMN IF NOT EXISTS "website" TEXT;
