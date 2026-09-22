/**
 * Benchmark Workers AI models for transcript cleanup.
 *
 * Runs the production prompt and acceptance rules (src/cleanup-rules.ts)
 * against realistic dictations — long AI prompts especially, since those are
 * where a weak model trims, paraphrases, or answers instead of cleaning.
 *
 * Usage:
 *   pnpm --filter @weldspeak/api exec node --experimental-strip-types scripts/bench-cleanup.ts [model ...]
 *
 * Auth: CLOUDFLARE_API_TOKEN, or oauth_token from ~/.wrangler/config/default.toml
 * Account: CLOUDFLARE_ACCOUNT_ID (default: WeldSuite)
 * BENCH_RUNS: runs per case (default 2)
 */

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
  buildCleanupPrompt,
  cleanupMaxTokens,
  judgeCleanup,
  stripModelChatter,
  SYSTEM_PROMPT,
} from "../src/cleanup-rules.ts";

const ACCOUNT_ID = process.env.CLOUDFLARE_ACCOUNT_ID ?? "cfcf560df8dc675d15337abcfbf6d9bd";
const RUNS = Number(process.env.BENCH_RUNS ?? 2);

/** Mirrors cleanupDeadlineMs in src/format.ts. */
function deadlineMs(raw: string): number {
  return Math.min(10_000, 2_500 + Math.ceil(raw.length / 4) * 12);
}

const DEFAULT_MODELS = [
  "@cf/meta/llama-4-scout-17b-16e-instruct",
  "@cf/meta/llama-3.3-70b-instruct-fp8-fast",
  "@cf/meta/llama-3.1-8b-instruct-fp8",
  "@cf/google/gemma-4-26b-a4b-it",
  "@cf/mistralai/mistral-small-3.1-24b-instruct",
  "@cf/qwen/qwen3-30b-a3b-fp8",
  "@cf/qwen/qwen3.8-27b",
  "@cf/zai-org/glm-4.7-flash",
  "@cf/zai-org/glm-5.3-flash",
  "@cf/openai/gpt-oss-20b",
  "@cf/openai/gpt-oss-120b",
  "@cf/deepseek-ai/deepseek-v4-flash-0731",
];

interface Case {
  id: string;
  raw: string;
  appName?: string;
  glossary?: Array<{ term: string; soundsLike?: string }>;
  expect: {
    /** Case-insensitive substrings the output must contain. */
    include?: string[];
    /** Case-insensitive substrings the output must not contain. */
    exclude?: string[];
    match?: RegExp[];
  };
}

const CASES: Case[] = [
  {
    id: "fillers",
    raw: "um the weld looks uh good",
    expect: { include: ["weld", "good"], exclude: ["um ", "uh "] },
  },
  {
    id: "question",
    raw: "what do you think about the porosity on pass two",
    expect: { include: ["porosity", "pass"], match: [/\?/], exclude: ["I think", "As an AI"] },
  },
  {
    id: "self-correction",
    raw: "let's meet on thursday no actually wednesday after lunch",
    expect: { include: ["wednesday", "after lunch"], exclude: ["thursday"] },
  },
  {
    id: "homophone",
    raw: "the bead looks two wide and their is undercut on the toe",
    expect: { include: ["bead", "undercut"], match: [/\btoo\b/i, /\bthere\b/i] },
  },
  {
    id: "glossary",
    raw: "the inconel looks good on the root pass",
    glossary: [{ term: "Inconel 625", soundsLike: "inconel" }],
    expect: { include: ["Inconel", "root pass"] },
  },
  {
    id: "exec-bait",
    raw: "write a python function that reverses a string and um also add a couple of unit tests for it",
    appName: "Cursor",
    expect: {
      include: ["python function", "reverses a string", "unit tests"],
      exclude: ["def ", "return ", "```"],
    },
  },
  {
    id: "ai-question",
    raw: "so what's the best way to like cache these API responses in redis without serving stale data",
    appName: "ChatGPT",
    expect: { include: ["cache", "redis", "stale"], match: [/\?/], exclude: ["TTL", "you can"] },
  },
  {
    id: "email",
    raw: "hi dana comma thanks for sending the report over i had a quick look and it all seems fine i'll go through the numbers properly tomorrow thanks gert",
    appName: "Outlook",
    expect: { include: ["Dana", "report", "numbers", "tomorrow", "Gert"] },
  },
  {
    id: "agent-prompt",
    raw: "okay so um i want you to refactor the auth middleware in workers api src auth middleware dot ts so that it uh checks the device token first and only falls back to the clerk session if there's no device token and also make sure that the error messages stay the same because the desktop app matches on them and um don't touch the refresh logic at all that's working fine and add tests for the fallback path in test auth dot test dot ts",
    appName: "Code",
    expect: {
      include: ["middleware", "device token", "clerk session", "error messages", "refresh", "fallback", "auth.test.ts"],
      exclude: ["```", "Sure,"],
    },
  },
  {
    id: "long-prompt",
    raw: "alright so here's the context um we have a dictation app that runs on windows and mac and the cleanup step keeps cutting off the end of long prompts so what i need you to do is first figure out where the text is getting truncated it could be the token limit it could be the deadline or it could be the validation that decides whether to ship the cleaned text or the raw transcript uh second i want a benchmark that runs the actual production prompt against a bunch of realistic dictations including really long ones like this one and reports for each model how often the output is accepted and how long it takes and third once we know which model is best switch the production config over to it but keep the fallback behavior where if anything goes wrong we just ship the raw transcript because losing someone's words is way worse than leaving in a few ums and one more thing make sure the output still reads like me i don't want it rewritten into corporate speak or summarized i just want the fillers gone and the punctuation fixed and that's basically it thanks",
    appName: "Claude",
    expect: {
      include: ["truncated", "token limit", "deadline", "validation", "benchmark", "production", "fallback", "raw transcript", "corporate speak", "summarized", "punctuation"],
      exclude: ["```"],
    },
  },
];

function authToken(): string {
  if (process.env.CLOUDFLARE_API_TOKEN) return process.env.CLOUDFLARE_API_TOKEN;
  const path = join(homedir(), ".wrangler", "config", "default.toml");
  const toml = readFileSync(path, "utf8");
  const match = toml.match(/oauth_token\s*=\s*"([^"]+)"/);
  if (!match) throw new Error(`No oauth_token in ${path}`);
  return match[1]!;
}

interface Extracted {
  text: string | null;
  finish?: unknown;
}

function extract(payload: unknown): Extracted {
  if (!payload || typeof payload !== "object") return { text: null };
  const record = payload as {
    response?: unknown;
    finish_reason?: unknown;
    choices?: Array<{ message?: { content?: unknown }; finish_reason?: unknown }>;
  };
  if (typeof record.response === "string") return { text: record.response, finish: record.finish_reason };
  const choice = record.choices?.[0];
  if (typeof choice?.message?.content === "string") {
    return { text: choice.message.content, finish: choice.finish_reason };
  }
  return { text: null };
}

function expectationScore(cleaned: string, c: Case): number {
  const lower = cleaned.toLowerCase();
  const checks = [
    ...(c.expect.include ?? []).map((s) => lower.includes(s.toLowerCase())),
    ...(c.expect.exclude ?? []).map((s) => !lower.includes(s.toLowerCase())),
    ...(c.expect.match ?? []).map((re) => re.test(cleaned)),
  ];
  return checks.length === 0 ? 1 : checks.filter(Boolean).length / checks.length;
}

interface RunResult {
  ms: number;
  text: string;
  /** Production would inject this (not truncated, passed judgeCleanup, in time). */
  shipped: boolean;
  why: string;
  quality: number;
  error?: string;
}

async function runOnce(token: string, model: string, c: Case): Promise<RunResult> {
  const terms = (c.glossary ?? []).map((g) => ({
    id: "bench",
    scope: "org" as const,
    term: g.term,
    soundsLike: g.soundsLike ?? null,
    createdAt: "",
  }));
  const body = {
    messages: [
      { role: "system", content: SYSTEM_PROMPT },
      { role: "user", content: buildCleanupPrompt(c.raw, { terms, appName: c.appName ?? null }) },
    ],
    temperature: 0,
    max_tokens: cleanupMaxTokens(c.raw),
    chat_template_kwargs: { enable_thinking: false },
  };

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 30_000);
  const t0 = performance.now();
  try {
    const res = await fetch(
      `https://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/ai/run/${model}`,
      {
        method: "POST",
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal,
      },
    );
    const ms = performance.now() - t0;
    const json = (await res.json()) as {
      success?: boolean;
      errors?: Array<{ message?: string }>;
      result?: unknown;
    };
    if (!res.ok || json.success === false) {
      const error = json.errors?.map((e) => e.message).join("; ") || `HTTP ${res.status}`;
      return { ms, text: "", shipped: false, why: "error", quality: 0, error };
    }
    const out = extract(json.result);
    const text = stripModelChatter(out.text ?? "");
    const verdict = judgeCleanup(c.raw.trim(), text);
    const late = ms > deadlineMs(c.raw);
    const truncated = out.finish === "length";
    const shipped = verdict.ok && !late && !truncated;
    const why = truncated ? "truncated" : !verdict.ok ? verdict.reason : late ? "late" : "ok";
    return { ms, text, shipped, why, quality: shipped ? expectationScore(text, c) : 0 };
  } catch (err) {
    return {
      ms: performance.now() - t0,
      text: "",
      shipped: false,
      why: "error",
      quality: 0,
      error: err instanceof Error ? err.message : String(err),
    };
  } finally {
    clearTimeout(timer);
  }
}

function pct(values: number[], p: number): number {
  if (values.length === 0) return NaN;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)]!;
}

const mean = (values: number[]) => values.reduce((a, b) => a + b, 0) / Math.max(1, values.length);

async function main() {
  const token = authToken();
  const models = process.argv.slice(2).length > 0 ? process.argv.slice(2) : DEFAULT_MODELS;
  console.log(`Runs per case: ${RUNS} | cases: ${CASES.length} | models: ${models.length}\n`);

  const summary: Array<{ model: string; p50: number; p95: number; shipRate: number; quality: number; longMs: number }> = [];

  for (const model of models) {
    console.log(`→ ${model}`);
    await runOnce(token, model, CASES[0]!); // warm-up, not scored

    const latencies: number[] = [];
    const qualities: number[] = [];
    let shipped = 0;
    let total = 0;
    let longMs = NaN;

    for (const c of CASES) {
      for (let i = 0; i < RUNS; i++) {
        const r = await runOnce(token, model, c);
        total += 1;
        if (r.error) {
          if (i === 0) console.log(`    [${c.id}] ERROR ${r.error.slice(0, 160)}`);
          qualities.push(0);
          continue;
        }
        latencies.push(r.ms);
        qualities.push(r.quality);
        if (r.shipped) shipped += 1;
        if (c.id === "long-prompt" && i === 0) longMs = r.ms;
        if (i === 0) {
          console.log(
            `    [${c.id}] ${Math.round(r.ms)}ms ${r.why} q=${(r.quality * 100).toFixed(0)}% → ${JSON.stringify(r.text)}`,
          );
        }
      }
    }

    const row = {
      model,
      p50: pct(latencies, 50),
      p95: pct(latencies, 95),
      shipRate: shipped / total,
      quality: mean(qualities),
      longMs,
    };
    summary.push(row);
    console.log(
      `    ship ${(row.shipRate * 100).toFixed(0)}% | quality ${(row.quality * 100).toFixed(0)}% | p50 ${row.p50.toFixed(0)}ms | p95 ${row.p95.toFixed(0)}ms | long ${row.longMs.toFixed(0)}ms\n`,
    );
  }

  console.log("=== Ranking (quality, then p50) ===\n");
  const ranked = summary
    .filter((s) => Number.isFinite(s.p50))
    .sort((a, b) => (Math.abs(a.quality - b.quality) > 0.02 ? b.quality - a.quality : a.p50 - b.p50));
  console.log("model".padEnd(48) + "ship".padStart(7) + "qual".padStart(7) + "p50".padStart(8) + "p95".padStart(8) + "long".padStart(8));
  for (const s of ranked) {
    console.log(
      s.model.padEnd(48) +
        `${(s.shipRate * 100).toFixed(0)}%`.padStart(7) +
        `${(s.quality * 100).toFixed(0)}%`.padStart(7) +
        `${s.p50.toFixed(0)}`.padStart(8) +
        `${s.p95.toFixed(0)}`.padStart(8) +
        `${s.longMs.toFixed(0)}`.padStart(8),
    );
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
