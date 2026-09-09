/**
 * Typed client for the WeldSpeak API.
 *
 * Every call carries a fresh Clerk session token. Clerk tokens are short-lived
 * by design, so `getToken()` is called per request rather than cached — the
 * Clerk SDK handles refresh transparently.
 */

import type {
  DictionaryTerm,
  MeResponse,
  OrgSettings,
  TranscriptRecord,
  UsageSummary,
} from "@weldspeak/protocol";

export type TokenGetter = () => Promise<string | null>;

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(
  getToken: TokenGetter,
  path: string,
  init: RequestInit & { orgId?: string | null } = {},
): Promise<T> {
  const token = await getToken();
  const headers = new Headers(init.headers);

  if (token) headers.set("Authorization", `Bearer ${token}`);
  if (init.orgId) headers.set("X-WeldSpeak-Org", init.orgId);
  if (init.body) headers.set("Content-Type", "application/json");

  const response = await fetch(path, { ...init, headers });

  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as {
      error?: string;
      message?: string;
    } | null;

    throw new ApiError(
      response.status,
      body?.error ?? "unknown",
      body?.message ?? response.statusText,
    );
  }

  return response.json() as Promise<T>;
}

export function createApi(getToken: TokenGetter, orgId: string | null) {
  const scoped = <T>(path: string, init: RequestInit = {}) =>
    request<T>(getToken, path, { ...init, orgId });

  return {
    me: () => scoped<MeResponse>("/api/me"),

    /** Approve a waiting desktop install. */
    approveDevice: (userCode: string) =>
      scoped<{ ok: true; device: { platform: string; label: string } }>(
        "/auth/device/approve",
        { method: "POST", body: JSON.stringify({ userCode }) },
      ),

    denyDevice: (userCode: string) =>
      scoped<{ ok: true }>("/auth/device/deny", {
        method: "POST",
        body: JSON.stringify({ userCode }),
      }),

    listTerms: () => scoped<{ terms: DictionaryTerm[] }>("/api/dictionary"),

    addTerm: (term: string, scope: "user" | "org", soundsLike?: string) =>
      scoped<DictionaryTerm>("/api/dictionary", {
        method: "POST",
        body: JSON.stringify({ term, scope, soundsLike }),
      }),

    deleteTerm: (id: string) =>
      scoped<{ ok: true }>(`/api/dictionary/${id}`, { method: "DELETE" }),

    listTranscripts: (query?: string) =>
      scoped<{ transcripts: TranscriptRecord[] }>(
        `/api/transcripts${query ? `?q=${encodeURIComponent(query)}` : ""}`,
      ),

    orgSettings: () => scoped<OrgSettings>("/api/org/settings"),

    updateOrgSettings: (patch: Partial<OrgSettings>) =>
      scoped<OrgSettings>("/api/org/settings", {
        method: "PATCH",
        body: JSON.stringify(patch),
      }),

    usage: () => scoped<UsageSummary>("/api/org/usage"),
  };
}

export type Api = ReturnType<typeof createApi>;
