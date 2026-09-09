/**
 * Secrets, declared alongside the bindings `wrangler types` generates.
 *
 * Bindings and vars come from wrangler.toml via worker-configuration.d.ts and
 * must not be hand-edited. Secrets are set with `wrangler secret put` and never
 * appear in configuration, so they are declared here and merged into the same
 * `Cloudflare.Env` interface.
 */
declare namespace Cloudflare {
  interface Env {
    /** Clerk backend API key (`sk_...`). */
    CLERK_SECRET_KEY: string;
    /** Clerk publishable key (`pk_...`). */
    CLERK_PUBLISHABLE_KEY: string;
    /** Svix signing secret for the Clerk webhook. */
    CLERK_WEBHOOK_SECRET: string;
    /** Random 32+ byte secret used to sign WeldSpeak access tokens. */
    TOKEN_SIGNING_KEY: string;
  }
}
