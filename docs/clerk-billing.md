!-- Clerk Billing (manual Dashboard setup)

Before production billing works, configure the shared WeldSuite Clerk app:

1. Enable **Billing for users** (B2C) under Billing Settings.
2. Connect Stripe (Clerk development gateway for test; your Stripe account for production).
3. Create a user plan with slug **`weldspeak`** at **$10 / month**.
4. Add feature **`unlimited_words`** to that plan (paid users are uncapped).
5. Create feature **`weldsuite`** (not attached to the paid plan). Grant it to WeldSuite-included accounts via the Dashboard (or set user `public_metadata.weldsuite = true`) so they get unlimited words without a second charge.

Code checks these exact slugs. See README “Billing” section.
