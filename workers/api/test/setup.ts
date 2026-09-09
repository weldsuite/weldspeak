import { applyD1Migrations, env } from "cloudflare:test";
import { installClerkInterceptor } from "./clerk-stub.js";

// Must run before any test file — and therefore before @clerk/backend — is
// imported: Clerk binds `fetch` at module load, so a later swap of the global
// would never be seen. See test/clerk-stub.ts.
installClerkInterceptor();

// Each test worker gets its own isolated D1 instance, so migrations run once
// per file rather than being shared and drifting between them.
await applyD1Migrations(env.DB, (env as unknown as { TEST_MIGRATIONS: [] }).TEST_MIGRATIONS);
