// Migrations are injected as a binding by vitest.config.ts and applied in
// test/setup.ts. Declaring it here keeps `env` fully typed in tests.
declare namespace Cloudflare {
  interface Env {
    TEST_MIGRATIONS: D1Migration[];
  }
}
