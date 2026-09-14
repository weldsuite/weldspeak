import { defineConfig } from "vitest/config";
import { cloudflareTest, readD1Migrations } from "@cloudflare/vitest-pool-workers";

// Migrations are read at config time and applied per test file (see test/setup.ts),
// so the schema under test is always the one that will actually be deployed.
const migrations = await readD1Migrations("./migrations");

export default defineConfig({
  plugins: [
    cloudflareTest({
      wrangler: { configPath: "./wrangler.test.toml" },
      miniflare: {
        // Layered over wrangler.toml so tests use obviously-fake secrets
        // rather than reaching for real ones.
        bindings: {
          TEST_MIGRATIONS: migrations,
          // @clerk/backend binds `fetch` at import time and only consults the
          // live global when NODE_ENV is "test". Without this the Clerk stub in
          // test/clerk-stub.ts is bypassed and the suite hits the real API.
          NODE_ENV: "test",
          APP_URL: "http://localhost:5173",
          CLEANUP_MODEL: "@cf/meta/llama-4-scout-17b-16e-instruct",
          STT_MODEL: "@cf/deepgram/nova-3",
          CLERK_SECRET_KEY: "sk_test_fake",
          CLERK_PUBLISHABLE_KEY: "pk_test_fake",
          CLERK_WEBHOOK_SECRET: "whsec_ZmFrZXNlY3JldGZha2VzZWNyZXRmYWtl",
          TOKEN_SIGNING_KEY: "test-signing-key-at-least-32-bytes-long!",
        },
      },
    }),
  ],
  test: {
    globals: true,
    setupFiles: ["./test/setup.ts"],
  },
});
