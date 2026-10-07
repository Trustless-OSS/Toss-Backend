ALTER TABLE "profiles" ADD COLUMN "website" TEXT;
ALTER TABLE "profiles" ALTER COLUMN "skills" TYPE TEXT USING array_to_string("skills", ', ');
