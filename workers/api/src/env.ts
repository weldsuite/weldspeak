/**
 * The Worker's environment.
 *
 * Bindings and vars are generated from wrangler.toml into
 * `worker-configuration.d.ts`; secrets are declared in `env.d.ts`. Both merge
 * into `Cloudflare.Env`, so this alias stays correct as bindings change —
 * there is no second list to keep in step.
 */
export type Env = Cloudflare.Env;
