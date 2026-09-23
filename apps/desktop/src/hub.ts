/**
 * WeldSpeak Hub — the window behind the tray icon (Home, Dictionary,
 * Snippets, Settings).
 *
 * Dictation, hotkeys, injection and the listening pill stay native. This
 * window is only history and preferences, so it is kept deliberately plain:
 * one render function, one delegated listener per event type, and state in a
 * single object. Re-rendering the whole tree is cheap at this size; inputs a
 * person is typing into are carried across renders by `data-keep`.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Page = "home" | "dictionary" | "snippets" | "settings";

interface Settings {
  apiBase: string;
  hotkey: { mode: "pushToTalk" | "toggle"; accelerator: string };
  orgId: string | null;
  injection: "automatic" | "alwaysType" | "alwaysPaste";
  cleanUpText: boolean;
  locale: string | null;
  useContext: boolean;
  keepHistory: boolean;
  pauseMedia: boolean;
  microphone: string | null;
  snippets: Array<{ trigger: string; expansion: string }>;
  wordsDictated: number;
}

interface Status {
  signedIn: boolean;
  email: string | null;
  orgs: Array<{ orgId: string; name: string; role: string }>;
  canInject: boolean;
}

interface Transcript {
  id: string;
  raw: string;
  formatted: string;
  durationMs: number;
  appName: string | null;
  createdAt: string;
}

interface DictionaryTerm {
  id: string;
  scope: string;
  term: string;
  soundsLike: string | null;
  createdAt: string;
}

interface Microphone {
  name: string;
  isDefault: boolean;
}

interface SignInStarted {
  verifyUrl: string;
  userCode: string;
}

interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  availableVersion: string | null;
}

type UpdatePhase = "checking" | "ready" | "installing" | "latest" | "error";

const LOCALES: Array<[string, string]> = [
  // No choice means English on the server. A saved "en" from before shows as
  // this entry too, since no other option matches it.
  ["", "English"],
  ["multi", "Mixed languages (less accurate)"],
  ["nl", "Dutch"],
  ["de", "German"],
  ["fr", "French"],
  ["es", "Spanish"],
  ["pt", "Portuguese"],
  ["it", "Italian"],
  ["pl", "Polish"],
  ["sv", "Swedish"],
  ["da", "Danish"],
  ["nb", "Norwegian"],
  ["fi", "Finnish"],
  ["tr", "Turkish"],
  ["ja", "Japanese"],
  ["ko", "Korean"],
  ["zh", "Chinese"],
  ["ar", "Arabic"],
  ["hi", "Hindi"],
];

/** How long the server may take to store a transcript after returning it. */
const PERSIST_GRACE_MS = 1_500;
/** Give up on a key capture nobody finishes. */
const BIND_TIMEOUT_MS = 12_000;
/** Refetch at most this often when the window regains focus. */
const FOCUS_REFRESH_MS = 30_000;

// --- Sample data for `pnpm dev` in a plain browser ---------------------------

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

const PREVIEW = {
  settings: {
    apiBase: "https://weldspeak.weldsuite.org",
    hotkey: { mode: "pushToTalk", accelerator: "ControlRight" },
    orgId: "org_preview",
    injection: "automatic",
    cleanUpText: true,
    locale: null,
    useContext: true,
    keepHistory: true,
    pauseMedia: true,
    microphone: null,
    snippets: [
      { trigger: "my address", expansion: "12 Harbour Lane, Rotterdam" },
      { trigger: "sign off", expansion: "Best regards,\nGert" },
    ],
    wordsDictated: 12_840,
  } satisfies Settings,
  status: {
    signedIn: true,
    email: "gert@weldsuite.org",
    orgs: [{ orgId: "org_preview", name: "WeldSuite", role: "admin" }],
    canInject: true,
  } satisfies Status,
  transcripts: [
    ["Can you send the Inconel quote to Marina before lunch?", 12, 4200],
    ["Schedule the fit-up inspection for Thursday morning.", 95, 5100],
    [
      "The heat input on pass three was a bit high, so let's drop the travel speed and check the interpass temperature before we continue.",
      60 * 26,
      9800,
    ],
    ["Order two more spools of ER70S-6.", 60 * 30, 2900],
  ].map(([text, ago, ms], i) => ({
    id: `t${i}`,
    raw: String(text),
    formatted: String(text),
    durationMs: Number(ms),
    appName: null,
    createdAt: minutesAgo(Number(ago)),
  })) as Transcript[],
  dictionary: [
    { id: "d1", scope: "org", term: "Inconel 625", soundsLike: "in co nel", createdAt: minutesAgo(1) },
    { id: "d2", scope: "user", term: "WeldSuite", soundsLike: null, createdAt: minutesAgo(1) },
    { id: "d3", scope: "user", term: "ER70S-6", soundsLike: "E R seventy S six", createdAt: minutesAgo(1) },
  ] as DictionaryTerm[],
  microphones: [
    { name: "Microphone (Shure MV7)", isDefault: true },
    { name: "Headset (Jabra Evolve2)", isDefault: false },
  ] as Microphone[],
};

// --- State -------------------------------------------------------------------

const state = {
  preview: false,
  page: "home" as Page,
  settings: PREVIEW.settings as Settings,
  status: { signedIn: false, email: null, orgs: [], canInject: true } as Status,
  transcripts: [] as Transcript[],
  transcriptsLoaded: false,
  dictionary: [] as DictionaryTerm[],
  microphones: [] as Microphone[],
  hotkeyLabel: "Right Ctrl",
  hotkeyWarning: null as string | null,
  binding: false,
  bindError: null as string | null,
  dictFilter: "",
  update: {
    phase: "checking" as UpdatePhase,
    info: { available: false, currentVersion: "", availableVersion: null } as UpdateInfo,
    error: null as string | null,
  },
  signIn: null as null | { code: string; url: string; error: string | null },
  pending: new Set<string>(),
};

let root: HTMLElement;
let toastTimer: number | undefined;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

// --- Boot --------------------------------------------------------------------

export async function mountHub(element: HTMLElement): Promise<void> {
  root = element;
  wireEvents();

  try {
    state.settings = await call<Settings>("get_settings");
  } catch {
    enterPreview();
    render();
    return;
  }

  render();
  await refreshAll();

  void listen("weldspeak://signed-in", async () => {
    state.signIn = null;
    await refreshAll();
    toast("Signed in");
  });
  void listen<string>("weldspeak://sign-in-failed", (event) => {
    if (state.signIn) {
      state.signIn.error = event.payload;
      render();
    }
  });
  void listen<{ text: string }>("weldspeak://history-changed", (event) => {
    void onDictated(event.payload?.text ?? "");
  });
  void listen<UpdateInfo>("weldspeak://update-status", (event) => {
    applyUpdate(event.payload);
    render();
  });
  // The window is hidden, not destroyed, when closed. Freshen it on return so
  // a Hub left open overnight does not still label yesterday as "Today".
  let lastFocusRefresh = Date.now();
  window.addEventListener("focus", () => {
    if (state.binding || Date.now() - lastFocusRefresh < FOCUS_REFRESH_MS) return;
    lastFocusRefresh = Date.now();
    void refreshAll();
  });

  void checkForUpdate();
}

function enterPreview(): void {
  state.preview = true;
  state.settings = PREVIEW.settings;
  state.status = PREVIEW.status;
  state.transcripts = PREVIEW.transcripts;
  state.transcriptsLoaded = true;
  state.dictionary = PREVIEW.dictionary;
  state.microphones = PREVIEW.microphones;
  state.update = {
    phase: "ready",
    info: { available: true, currentVersion: "0.1.20", availableVersion: "0.1.24" },
    error: null,
  };
}

async function refreshAll(): Promise<void> {
  if (state.preview) return;
  const [settings, status] = await Promise.all([
    call<Settings>("get_settings").catch(() => state.settings),
    call<Status>("get_status").catch(
      (): Status => ({ signedIn: false, email: null, orgs: [], canInject: true }),
    ),
  ]);
  state.settings = settings;
  state.status = status;
  await Promise.all([
    refreshTranscripts(),
    refreshDictionary(),
    refreshMicrophones(),
    refreshHotkey(),
  ]);
  render();
}

async function refreshTranscripts(): Promise<void> {
  if (state.preview) return;
  if (!state.status.signedIn) {
    state.transcripts = [];
    state.transcriptsLoaded = true;
    return;
  }
  try {
    state.transcripts = await call<Transcript[]>("list_transcripts");
  } catch {
    // Keep what we had; a flaky network should not blank the history.
  }
  state.transcriptsLoaded = true;
}

async function refreshDictionary(): Promise<void> {
  if (state.preview) return;
  if (!state.status.signedIn) {
    state.dictionary = [];
    return;
  }
  try {
    state.dictionary = await call<DictionaryTerm[]>("list_dictionary");
  } catch {
    // As above.
  }
}

async function refreshMicrophones(): Promise<void> {
  if (state.preview) return;
  state.microphones = await call<Microphone[]>("list_microphones").catch(() => []);
}

async function refreshHotkey(): Promise<void> {
  const accelerator = state.settings.hotkey.accelerator;
  if (state.preview) {
    state.hotkeyLabel = fallbackKeyLabel(accelerator);
    return;
  }
  const [label, warning] = await Promise.all([
    call<string>("hotkey_label", { accelerator }).catch(() => fallbackKeyLabel(accelerator)),
    call<string | null>("hotkey_warning", { accelerator }).catch(() => null),
  ]);
  state.hotkeyLabel = label;
  state.hotkeyWarning = warning;
}

/**
 * A dictation was just inserted.
 *
 * The server replies with the text before it has written the transcript, so
 * an immediate refetch usually misses the newest row. Show it straight away
 * from what the app already knows, then reconcile with the server.
 */
async function onDictated(text: string): Promise<void> {
  const words = text.split(/\s+/).filter(Boolean).length;
  state.settings = { ...state.settings, wordsDictated: state.settings.wordsDictated + words };
  if (text && state.status.signedIn && state.settings.keepHistory) {
    state.transcripts = [
      {
        id: `local-${Date.now()}`,
        raw: text,
        formatted: text,
        durationMs: 0,
        appName: null,
        createdAt: new Date().toISOString(),
      },
      ...state.transcripts,
    ];
  }
  render();

  window.setTimeout(async () => {
    state.settings = await call<Settings>("get_settings").catch(() => state.settings);
    await refreshTranscripts();
    render();
  }, PERSIST_GRACE_MS);
}

// --- Rendering ---------------------------------------------------------------

let renderedPage: Page | null = null;
let dialogWasOpen = false;

function render(): void {
  const kept = captureInputs();
  const toastEl = root.querySelector("#toast");
  // Entrance motion only when something actually appears, not on every
  // re-render caused by a toggle or a background refresh.
  const entering = renderedPage !== state.page;
  const dialogEntering = Boolean(state.signIn) && !dialogWasOpen;
  root.innerHTML = `
    <div class="app">
      ${sidebar()}
      <main class="panel" data-page="${state.page}">
        <div class="panel-scroll" id="panel-scroll">
          <div class="page${entering ? " is-entering" : ""}">${page()}</div>
        </div>
      </main>
    </div>
    ${state.signIn ? signInDialog(dialogEntering) : ""}
  `;
  // The toast outlives renders so a message is not cut off by the refresh
  // that follows the action it reports.
  root.appendChild(toastEl ?? createToast());
  renderedPage = state.page;
  dialogWasOpen = Boolean(state.signIn);
  restoreInputs(kept);
}

function createToast(): HTMLElement {
  const el = document.createElement("div");
  el.className = "toast";
  el.id = "toast";
  el.setAttribute("role", "status");
  el.setAttribute("aria-live", "polite");
  return el;
}

interface Kept {
  values: Map<string, string>;
  focus: string | null;
  selection: [number, number] | null;
  scroll: number;
  page: Page;
}

function captureInputs(): Kept {
  const values = new Map<string, string>();
  root.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-keep]").forEach((el) => {
    values.set(el.dataset.keep!, el.value);
  });
  const active = document.activeElement as HTMLInputElement | null;
  const focus = active?.dataset?.keep ?? null;
  const selection =
    focus && active && typeof active.selectionStart === "number"
      ? ([active.selectionStart, active.selectionEnd ?? active.selectionStart] as [number, number])
      : null;
  const scroll = root.querySelector("#panel-scroll")?.scrollTop ?? 0;
  const page = (root.querySelector<HTMLElement>(".panel")?.dataset.page as Page) ?? state.page;
  return { values, focus, selection, scroll, page };
}

function restoreInputs(kept: Kept): void {
  kept.values.forEach((value, key) => {
    const el = root.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[data-keep="${key}"]`);
    if (el) el.value = value;
  });
  if (kept.focus) {
    const el = root.querySelector<HTMLInputElement>(`[data-keep="${kept.focus}"]`);
    el?.focus();
    if (el && kept.selection) el.setSelectionRange(kept.selection[0], kept.selection[1]);
  }
  // Only keep the scroll position when the same page is re-rendered in place.
  if (kept.page === state.page) {
    const scroller = root.querySelector("#panel-scroll");
    if (scroller) scroller.scrollTop = kept.scroll;
  }
}

function sidebar(): string {
  return `
    <aside class="sidebar">
      <div class="brand">
        ${logo()}
        <span>WeldSpeak</span>
      </div>
      <nav class="nav" aria-label="Sections">
        ${navItem("home", "Home", ICONS.home)}
        ${navItem("dictionary", "Dictionary", ICONS.book)}
        ${navItem("snippets", "Snippets", ICONS.zap)}
      </nav>
      <div class="sidebar-foot">
        ${sidebarUpdate()}
        ${navItem("settings", "Settings", ICONS.settings)}
      </div>
    </aside>
  `;
}

function navItem(id: Page, label: string, icon: string): string {
  const active = state.page === id;
  return `
    <button type="button" class="nav-item${active ? " is-active" : ""}" data-nav="${id}"${
      active ? ' aria-current="page"' : ""
    }>
      ${icon}<span>${label}</span>
    </button>
  `;
}

function sidebarUpdate(): string {
  const { phase, info } = state.update;
  if (phase === "installing") {
    return `<div class="update-card is-busy"><span class="spinner"></span><span>Installing update…</span></div>`;
  }
  if (phase !== "ready" || !info.availableVersion) return "";
  return `
    <div class="update-card">
      <div>
        <strong>Update ready</strong>
        <span>Version ${esc(info.availableVersion)}</span>
      </div>
      <button type="button" class="btn primary small" data-action="install-update">Install</button>
    </div>
  `;
}

function page(): string {
  switch (state.page) {
    case "home":
      return homePage();
    case "dictionary":
      return dictionaryPage();
    case "snippets":
      return snippetsPage();
    case "settings":
      return settingsPage();
  }
}

// --- Home --------------------------------------------------------------------

function homePage(): string {
  const { signedIn } = state.status;
  return `
    <header class="page-head">
      <div>
        <h1 class="display">${signedIn ? "Welcome back" : "Welcome to WeldSpeak"}</h1>
      </div>
      ${signedIn ? stats() : ""}
    </header>
    ${signedIn ? howToCard() : signInCard()}
    ${signedIn ? history() : ""}
  `;
}

function stats(): string {
  const wpm = averageWpm(state.transcripts);
  return `
    <dl class="stats">
      <div class="stat">
        <dt>Words</dt>
        <dd>${formatNumber(state.settings.wordsDictated)}</dd>
      </div>
      ${
        wpm
          ? `<div class="stat" title="Average over your recent dictations">
               <dt>WPM</dt>
               <dd>${wpm}</dd>
             </div>`
          : ""
      }
    </dl>
  `;
}

function howToCard(): string {
  return `
    <section class="hero">
      <div class="hero-copy">
        <h2>Hold ${keycaps(state.hotkeyLabel)} and speak in any app</h2>
        <p>Let go and your words appear wherever you were typing. Double-tap to keep
        listening hands-free, tap again to finish. <kbd class="kbd-inline">Esc</kbd> cancels.</p>
      </div>
      <div class="hero-wave" aria-hidden="true">
        ${Array.from({ length: 9 }, (_, i) => `<span style="--i:${i}"></span>`).join("")}
      </div>
    </section>
  `;
}

function signInCard(): string {
  return `
    <section class="hero hero-signin">
      <div class="hero-copy">
        <h2>Sign in to start dictating</h2>
        <p>Hold a key, speak, let go. Clean text lands in Slack, email, your IDE — wherever
        you were typing. Sign in with your WeldSuite account to begin.</p>
        <div class="hero-actions">
          <button type="button" class="btn primary" data-action="sign-in">Sign in</button>
        </div>
      </div>
      <div class="hero-wave" aria-hidden="true">
        ${Array.from({ length: 9 }, (_, i) => `<span style="--i:${i}"></span>`).join("")}
      </div>
    </section>
  `;
}

function history(): string {
  if (!state.transcriptsLoaded) {
    return `<div class="history"><div class="skeleton"></div><div class="skeleton"></div></div>`;
  }
  if (!state.settings.keepHistory && state.transcripts.length === 0) {
    return emptyState(
      ICONS.history,
      "History is off",
      "Turn on “Keep my dictations” in Settings to see them here.",
    );
  }
  if (state.transcripts.length === 0) {
    return emptyState(
      ICONS.mic,
      "Nothing here yet",
      `Hold ${esc(state.hotkeyLabel)} in any text box and start talking. Your dictations will show up here.`,
    );
  }

  const groups = new Map<string, Transcript[]>();
  for (const row of state.transcripts) {
    const key = dayLabel(parseServerDate(row.createdAt));
    const list = groups.get(key) ?? [];
    list.push(row);
    groups.set(key, list);
  }

  return `
    <div class="history">
      ${[...groups]
        .map(
          ([label, rows]) => `
        <section class="history-day">
          <h3 class="section-label">${esc(label)}</h3>
          <ul class="list">
            ${rows.map(historyRow).join("")}
          </ul>
        </section>`,
        )
        .join("")}
    </div>
  `;
}

function historyRow(row: Transcript): string {
  const text = row.formatted || row.raw;
  const date = parseServerDate(row.createdAt);
  const busy = state.pending.has(row.id);
  const local = row.id.startsWith("local-");
  return `
    <li class="history-row${busy ? " is-busy" : ""}">
      <time datetime="${esc(date.toISOString())}" title="${esc(date.toLocaleString())}">${esc(
        formatTime(date),
      )}</time>
      <p class="history-text">${esc(text)}</p>
      <div class="row-actions">
        ${iconButton("copy", ICONS.copy, "Copy", `data-text="${esc(text)}"`)}
        ${local ? "" : iconButton("delete-transcript", ICONS.trash, "Delete", `data-id="${esc(row.id)}"`, "danger")}
      </div>
    </li>
  `;
}

// --- Dictionary --------------------------------------------------------------

function dictionaryPage(): string {
  const head = pageHead(
    "Dictionary",
    "Names, jargon and part numbers WeldSpeak should always spell right.",
  );
  if (!state.status.signedIn) return head + signedOutState("Sign in to build your dictionary.");

  const filter = state.dictFilter.trim().toLowerCase();
  const terms = filter
    ? state.dictionary.filter((t) =>
        `${t.term} ${t.soundsLike ?? ""}`.toLowerCase().includes(filter),
      )
    : state.dictionary;

  return `
    ${head}
    <form class="composer" data-form="term" autocomplete="off">
      <input name="term" data-keep="term" placeholder="Add a word or name, e.g. Inconel 625" required maxlength="120" />
      <input name="sounds" data-keep="sounds" placeholder="Sounds like (optional)" maxlength="120" />
      <button type="submit" class="btn primary">${ICONS.plus}Add</button>
    </form>
    ${
      state.dictionary.length > 6
        ? `<label class="search">
             ${ICONS.search}
             <input type="search" data-keep="dict-filter" data-input="dict-filter" placeholder="Search ${state.dictionary.length} words" value="${esc(state.dictFilter)}" />
           </label>`
        : ""
    }
    ${
      state.dictionary.length === 0
        ? emptyState(
            ICONS.book,
            "Your dictionary is empty",
            "Add words the recognizer keeps getting wrong. Edits you make right after dictating are learned here automatically.",
          )
        : terms.length === 0
          ? `<p class="muted pad">No words match “${esc(state.dictFilter)}”.</p>`
          : `<ul class="list">${terms.map(termRow).join("")}</ul>`
    }
  `;
}

function termRow(term: DictionaryTerm): string {
  const team = term.scope === "org";
  return `
    <li class="term-row${state.pending.has(term.id) ? " is-busy" : ""}">
      <div class="term-copy">
        <span class="term">${esc(term.term)}</span>
        ${term.soundsLike ? `<span class="muted">sounds like “${esc(term.soundsLike)}”</span>` : ""}
      </div>
      ${team ? `<span class="badge" title="Shared with your organization">Team</span>` : ""}
      <div class="row-actions">
        ${iconButton("delete-term", ICONS.trash, "Remove", `data-id="${esc(term.id)}"`, "danger")}
      </div>
    </li>
  `;
}

// --- Snippets ----------------------------------------------------------------

function snippetsPage(): string {
  const snippets = state.settings.snippets ?? [];
  return `
    ${pageHead("Snippets", "Say a short cue and WeldSpeak types the full text for you.")}
    <form class="composer composer-snippet" data-form="snippet" autocomplete="off">
      <input name="trigger" data-keep="trigger" placeholder="Cue, e.g. my address" required maxlength="60" />
      <textarea name="expansion" data-keep="expansion" placeholder="Text to insert" required maxlength="4000" rows="1"></textarea>
      <button type="submit" class="btn primary">${ICONS.plus}Add</button>
    </form>
    ${
      snippets.length === 0
        ? emptyState(
            ICONS.zap,
            "No snippets yet",
            "Save things you type often: your address, a sign-off, a booking link.",
          )
        : `<ul class="list">
            ${snippets
              .map(
                (snip, index) => `
              <li class="snippet-row">
                <span class="cue">${esc(snip.trigger)}</span>
                <span class="arrow" aria-hidden="true">${ICONS.arrow}</span>
                <p class="expansion">${esc(snip.expansion)}</p>
                <div class="row-actions">
                  ${iconButton("delete-snippet", ICONS.trash, "Delete", `data-index="${index}"`, "danger")}
                </div>
              </li>`,
              )
              .join("")}
          </ul>`
    }
  `;
}

// --- Settings ----------------------------------------------------------------

function settingsPage(): string {
  const s = state.settings;
  const { status } = state;

  const micOptions = [
    option("", "System default", !s.microphone),
    ...state.microphones.map((mic) =>
      option(mic.name, mic.isDefault ? `${mic.name} (default)` : mic.name, s.microphone === mic.name),
    ),
  ];
  // A saved microphone that is unplugged right now should still show as chosen,
  // not silently flip the select to "System default".
  if (s.microphone && !state.microphones.some((m) => m.name === s.microphone)) {
    micOptions.push(option(s.microphone, `${s.microphone} (not connected)`, true));
  }

  return `
    ${pageHead("Settings")}

    ${status.canInject ? "" : accessibilityCard()}

    <section class="card">
      <h2 class="card-title">Account</h2>
      ${
        status.signedIn
          ? `<div class="setting">
               <div class="account">
                 <span class="avatar" aria-hidden="true">${esc(initial(status.email))}</span>
                 <div>
                   <strong>${esc(status.email ?? "Signed in")}</strong>
                   <span class="muted">${esc(status.orgs.map((o) => o.name).join(", ") || "Personal account")}</span>
                 </div>
               </div>
               <div class="setting-control">
                 <button type="button" class="btn" data-action="dashboard">Web dashboard ${ICONS.external}</button>
                 <button type="button" class="btn ghost" data-action="sign-out">Sign out</button>
               </div>
             </div>`
          : `<div class="setting">
               <div class="setting-copy">
                 <strong>Not signed in</strong>
                 <span>Sign in opens your browser and shows a short code to confirm.</span>
               </div>
               <div class="setting-control">
                 <button type="button" class="btn primary" data-action="sign-in">Sign in</button>
               </div>
             </div>`
      }
    </section>

    <section class="card">
      <h2 class="card-title">Dictation</h2>
      <div class="setting">
        <div class="setting-copy">
          <strong>Listen key</strong>
          <span>${
            state.binding
              ? "Hold the key or two keys you want to use… <kbd class='kbd-inline'>Esc</kbd> to cancel."
              : "Hold to talk, double-tap for hands-free. One key or two together."
          }</span>
          ${
            state.bindError
              ? `<span class="error">${esc(state.bindError)}</span>`
              : state.hotkeyWarning && !state.binding
                ? `<span class="warn">${esc(state.hotkeyWarning)}</span>`
                : ""
          }
        </div>
        <div class="setting-control">
          ${
            state.binding
              ? `<span class="listening"><span class="pulse"></span>Listening for keys</span>
                 <button type="button" class="btn ghost" data-action="cancel-bind">Cancel</button>`
              : `${keycaps(state.hotkeyLabel)}
                 <button type="button" class="btn" data-action="bind">Change</button>`
          }
        </div>
      </div>
      ${selectSetting("Microphone", "microphone", micOptions.join(""))}
      ${selectSetting(
        "Language",
        "locale",
        LOCALES.map(([v, l]) => option(v, l, (s.locale ?? "") === v)).join(""),
        "Pick one if WeldSpeak mishears your accent.",
      )}
      ${
        status.orgs.length > 0
          ? selectSetting(
              "Organization",
              "orgId",
              // No org chosen means dictations run under the personal account;
              // showing the first org as selected misrepresented that.
              [
                option("", "Personal", !s.orgId),
                ...status.orgs.map((o) => option(o.orgId, o.name, s.orgId === o.orgId)),
              ].join(""),
              "Its shared dictionary and policies apply to your dictations.",
            )
          : ""
      }
    </section>

    <section class="card">
      <h2 class="card-title">Text</h2>
      ${switchSetting(
        "Clean up what I say",
        "cleanUpText",
        s.cleanUpText,
        "Removes ums, false starts and repeats, and fixes punctuation. Off inserts exactly what you said.",
      )}
      ${selectSetting(
        "Insert text by",
        "injection",
        [
          option("automatic", "Choosing automatically", s.injection === "automatic"),
          option("alwaysType", "Typing it", s.injection === "alwaysType"),
          option("alwaysPaste", "Pasting it", s.injection === "alwaysPaste"),
        ].join(""),
        "Automatic pastes the whole dictation at once and puts your clipboard back. Choose typing for apps that block paste.",
      )}
      ${switchSetting(
        "Use what’s around my cursor",
        "useContext",
        s.useContext,
        "Reads the text next to your cursor and the window title so dictation continues your sentence, spells names on screen, and fits the app. Used for cleanup only, never stored.",
      )}
    </section>

    <section class="card">
      <h2 class="card-title">While dictating</h2>
      ${switchSetting(
        "Pause music and videos",
        "pauseMedia",
        s.pauseMedia,
        "Pauses or mutes other audio while you hold the key, then resumes it.",
      )}
      ${switchSetting(
        "Keep my dictations",
        "keepHistory",
        s.keepHistory,
        "Show recent dictations on Home. Your organization’s admin can turn this off for everyone.",
      )}
    </section>

    <section class="card">
      <h2 class="card-title">About</h2>
      <div class="setting">
        <div class="setting-copy">
          <strong>WeldSpeak ${esc(state.update.info.currentVersion || "")}</strong>
          <span>${updateLine()}</span>
        </div>
        <div class="setting-control">${updateControl()}</div>
      </div>
    </section>
  `;
}

function updateLine(): string {
  const { phase, info, error } = state.update;
  switch (phase) {
    case "checking":
      return "Checking for updates…";
    case "ready":
      return `Version ${esc(info.availableVersion ?? "")} is ready to install.`;
    case "installing":
      return "Downloading and installing. WeldSpeak restarts when it’s done.";
    case "error":
      return `<span class="error">${esc(error ?? "Could not check for updates.")}</span>`;
    case "latest":
      return "You’re on the latest version.";
  }
}

function updateControl(): string {
  switch (state.update.phase) {
    case "ready":
      return `<button type="button" class="btn primary" data-action="install-update">Install update</button>`;
    case "checking":
    case "installing":
      return `<span class="spinner"></span>`;
    default:
      return `<button type="button" class="btn" data-action="check-update">Check for updates</button>`;
  }
}

function accessibilityCard(): string {
  return `
    <section class="card card-warning">
      <div class="setting">
        <div class="setting-copy">
          <strong>Allow WeldSpeak to type for you</strong>
          <span>macOS needs Accessibility permission before WeldSpeak can insert text into other apps. Until then, dictations are copied to the clipboard.</span>
        </div>
        <div class="setting-control">
          <button type="button" class="btn primary" data-action="grant">Open settings</button>
        </div>
      </div>
    </section>
  `;
}

// --- Sign-in dialog ----------------------------------------------------------

function signInDialog(entering: boolean): string {
  const s = state.signIn!;
  return `
    <div class="scrim${entering ? " is-entering" : ""}" data-action="close-sign-in">
      <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="sign-in-title" data-stop>
        <h2 class="display" id="sign-in-title">Confirm in your browser</h2>
        <p class="muted">We opened WeldSpeak in your browser. Check it shows this code, then approve this computer.</p>
        <div class="code" aria-label="Confirmation code">${esc(s.code)}</div>
        ${
          s.error
            ? `<p class="error">${esc(s.error)}</p>`
            : `<p class="waiting"><span class="spinner"></span>Waiting for you to approve…</p>`
        }
        <div class="dialog-actions">
          <button type="button" class="btn ghost" data-action="close-sign-in">Close</button>
          ${
            s.error
              ? `<button type="button" class="btn primary" data-action="sign-in">Try again</button>`
              : `<button type="button" class="btn" data-action="reopen-browser">Open browser again ${ICONS.external}</button>`
          }
        </div>
      </div>
    </div>
  `;
}

// --- Small builders ----------------------------------------------------------

function pageHead(title: string, lede?: string): string {
  return `
    <header class="page-head">
      <div>
        <h1 class="display">${esc(title)}</h1>
        ${lede ? `<p class="lede">${esc(lede)}</p>` : ""}
      </div>
    </header>
  `;
}

function emptyState(icon: string, title: string, body: string): string {
  return `
    <div class="empty">
      <span class="empty-icon">${icon}</span>
      <strong>${esc(title)}</strong>
      <p>${body}</p>
    </div>
  `;
}

function signedOutState(message: string): string {
  return `
    <div class="empty">
      <span class="empty-icon">${ICONS.lock}</span>
      <strong>${esc(message)}</strong>
      <button type="button" class="btn primary" data-action="sign-in">Sign in</button>
    </div>
  `;
}

function iconButton(
  action: string,
  icon: string,
  label: string,
  attrs = "",
  tone = "",
): string {
  return `<button type="button" class="icon-btn ${tone}" data-action="${action}" ${attrs} title="${esc(label)}" aria-label="${esc(label)}">${icon}</button>`;
}

function switchSetting(label: string, key: keyof Settings, on: boolean, help: string): string {
  const id = `set-${key}`;
  return `
    <div class="setting">
      <div class="setting-copy">
        <label for="${id}"><strong>${esc(label)}</strong></label>
        <span>${esc(help)}</span>
      </div>
      <div class="setting-control">
        <label class="switch">
          <input type="checkbox" role="switch" id="${id}" data-setting="${key}" ${on ? "checked" : ""} />
          <span class="track"><span class="thumb"></span></span>
        </label>
      </div>
    </div>
  `;
}

function selectSetting(label: string, key: keyof Settings, options: string, help = ""): string {
  const id = `set-${key}`;
  return `
    <div class="setting">
      <div class="setting-copy">
        <label for="${id}"><strong>${esc(label)}</strong></label>
        ${help ? `<span>${esc(help)}</span>` : ""}
      </div>
      <div class="setting-control">
        <div class="select">
          <select id="${id}" data-setting="${key}">${options}</select>
          ${ICONS.chevron}
        </div>
      </div>
    </div>
  `;
}

function option(value: string, label: string, selected: boolean): string {
  return `<option value="${esc(value)}"${selected ? " selected" : ""}>${esc(label)}</option>`;
}

function keycaps(label: string): string {
  return `<span class="keys">${label
    .split(" + ")
    .map((part) => `<kbd>${esc(part)}</kbd>`)
    .join("")}</span>`;
}

function logo(): string {
  return `
    <svg class="logo" viewBox="0 0 28 28" aria-hidden="true">
      <rect width="28" height="28" rx="8" fill="currentColor"/>
      <g stroke="var(--logo-ink)" stroke-width="2.4" stroke-linecap="round">
        <path d="M8 12v4"/><path d="M12 9v10"/><path d="M16 11v6"/><path d="M20 13v2"/>
      </g>
    </svg>
  `;
}

// --- Events ------------------------------------------------------------------

function wireEvents(): void {
  root.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;

    const nav = target.closest<HTMLElement>("[data-nav]");
    if (nav) {
      if (state.binding) void endBind();
      state.page = nav.dataset.nav as Page;
      render();
      return;
    }

    const actionEl = target.closest<HTMLElement>("[data-action]");
    if (!actionEl) return;
    // Clicks inside the dialog must not reach the scrim's close action.
    if (actionEl.classList.contains("scrim") && target.closest("[data-stop]")) return;
    void onAction(actionEl.dataset.action!, actionEl);
  });

  root.addEventListener("change", (event) => {
    const el = event.target as HTMLInputElement | HTMLSelectElement;
    const key = el.dataset.setting as keyof Settings | undefined;
    if (!key) return;
    const value =
      el instanceof HTMLInputElement && el.type === "checkbox" ? el.checked : el.value || null;
    void saveSetting(key, value);
  });

  root.addEventListener("input", (event) => {
    const el = event.target as HTMLInputElement;
    if (el.dataset.input === "dict-filter") {
      state.dictFilter = el.value;
      render();
    }
  });

  root.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    if (form.dataset.form === "term") void addTerm(form);
    if (form.dataset.form === "snippet") void addSnippet(form);
  });

  // Enter in the snippet text area adds a new line; Ctrl/⌘+Enter submits.
  root.addEventListener("keydown", (event) => {
    const el = event.target as HTMLElement;
    if (el.tagName === "TEXTAREA" && event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      (el.closest("form") as HTMLFormElement | null)?.requestSubmit();
    }
  });

  window.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    if (state.binding) {
      event.preventDefault();
      void endBind();
    } else if (state.signIn) {
      state.signIn = null;
      render();
    }
  });
}

async function onAction(action: string, el: HTMLElement): Promise<void> {
  switch (action) {
    case "sign-in":
      return signIn();
    case "reopen-browser":
      if (state.signIn) void call("open_external_url", { url: state.signIn.url }).catch(() => {});
      return;
    case "close-sign-in":
      state.signIn = null;
      render();
      return;
    case "sign-out":
      await call("sign_out").catch(() => {});
      state.status = { ...state.status, signedIn: false, email: null, orgs: [] };
      state.transcripts = [];
      state.dictionary = [];
      render();
      return;
    case "dashboard":
      void call("open_external_url", {
        url: `${state.settings.apiBase.replace(/\/+$/, "")}/dictionary`,
      }).catch((error) => toast(String(error), "error"));
      return;
    case "grant":
      void call("open_permission_settings").catch(() => {});
      return;
    case "copy":
      return copy(el.dataset.text ?? "");
    case "delete-transcript":
      return deleteTranscript(el.dataset.id!);
    case "delete-term":
      return deleteTerm(el.dataset.id!);
    case "delete-snippet":
      return deleteSnippet(Number(el.dataset.index));
    case "bind":
      return startBind();
    case "cancel-bind":
      return endBind();
    case "check-update":
      return checkForUpdate();
    case "install-update":
      return installUpdate();
  }
}

async function signIn(): Promise<void> {
  if (state.preview) {
    state.signIn = { code: "WXYZ-2468", url: "https://example.com", error: null };
    render();
    return;
  }
  try {
    const started = await call<SignInStarted>("begin_sign_in");
    state.signIn = { code: started.userCode, url: started.verifyUrl, error: null };
  } catch (error) {
    toast(errorText(error, "Could not start sign-in. Check your connection."), "error");
  }
  render();
}

async function copy(text: string): Promise<void> {
  if (!text) return;
  try {
    if (state.preview) await navigator.clipboard.writeText(text);
    else await call("copy_text", { text });
    toast("Copied to clipboard");
  } catch (error) {
    toast(errorText(error, "Could not copy."), "error");
  }
}

async function deleteTranscript(id: string): Promise<void> {
  await removeWithUndo(
    id,
    () => (state.transcripts = state.transcripts.filter((t) => t.id !== id)),
    () => call("delete_transcript", { id }),
    refreshTranscripts,
  );
}

async function deleteTerm(id: string): Promise<void> {
  await removeWithUndo(
    id,
    () => (state.dictionary = state.dictionary.filter((t) => t.id !== id)),
    () => call("delete_dictionary_term", { id }),
    refreshDictionary,
  );
}

/** Optimistic removal: the row disappears at once and comes back if the server refuses. */
async function removeWithUndo(
  id: string,
  removeLocally: () => void,
  removeRemotely: () => Promise<unknown>,
  reload: () => Promise<void>,
): Promise<void> {
  if (state.pending.has(id)) return;
  state.pending.add(id);
  render();
  try {
    if (!state.preview) await removeRemotely();
    removeLocally();
  } catch (error) {
    toast(errorText(error, "Could not delete that."), "error");
    await reload();
  } finally {
    state.pending.delete(id);
    render();
  }
}

async function addTerm(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  const term = String(data.get("term") ?? "").trim();
  const sounds = String(data.get("sounds") ?? "").trim();
  if (!term) return;
  if (state.dictionary.some((t) => t.term.toLowerCase() === term.toLowerCase())) {
    toast(`“${term}” is already in your dictionary`);
    return;
  }
  const button = form.querySelector("button");
  if (button) button.disabled = true;
  try {
    const added = state.preview
      ? { id: `d${Date.now()}`, scope: "user", term, soundsLike: sounds || null, createdAt: "" }
      : await call<DictionaryTerm>("add_dictionary_term", { term, soundsLike: sounds || null });
    state.dictionary = [added, ...state.dictionary.filter((t) => t.id !== added.id)];
    clearKept(form);
    toast(`Added “${term}”`);
  } catch (error) {
    toast(errorText(error, "Could not add that word."), "error");
  }
  render();
  root.querySelector<HTMLInputElement>('[data-keep="term"]')?.focus();
}

async function addSnippet(form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  const trigger = String(data.get("trigger") ?? "").trim();
  const expansion = String(data.get("expansion") ?? "").trim();
  if (!trigger || !expansion) return;
  const next = [
    ...state.settings.snippets.filter((s) => s.trigger.toLowerCase() !== trigger.toLowerCase()),
    { trigger, expansion },
  ];
  if (await saveSetting("snippets", next)) {
    clearKept(form);
    toast(`Saved “${trigger}”`);
    render();
    root.querySelector<HTMLInputElement>('[data-keep="trigger"]')?.focus();
  }
}

async function deleteSnippet(index: number): Promise<void> {
  const next = state.settings.snippets.filter((_, i) => i !== index);
  await saveSetting("snippets", next);
}

function clearKept(form: HTMLFormElement): void {
  form.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("[data-keep]").forEach((el) => {
    el.value = "";
  });
}

async function saveSetting(key: keyof Settings, value: unknown): Promise<boolean> {
  const previous = state.settings;
  state.settings = { ...state.settings, [key]: value } as Settings;
  try {
    if (!state.preview) {
      state.settings = await call<Settings>("update_settings", { patch: { [key]: value } });
    }
    if (key === "orgId" && !state.preview) {
      // A different organization has a different shared dictionary.
      await refreshDictionary();
    }
    render();
    return true;
  } catch (error) {
    state.settings = previous;
    toast(errorText(error, "Could not save that setting."), "error");
    render();
    return false;
  }
}

// --- Listen-key capture ------------------------------------------------------

let bindToken = 0;

async function startBind(): Promise<void> {
  if (state.binding) return;
  const token = ++bindToken;
  state.binding = true;
  state.bindError = null;
  render();
  await call("suspend_hotkey", { paused: true }).catch(() => {});

  const started = Date.now();
  const tick = async (): Promise<void> => {
    if (token !== bindToken) return;
    let code: string | null = null;
    try {
      code = await call<string | null>("poll_held_hotkey");
    } catch {
      // Browser preview: pretend a two-key chord was pressed.
      if (Date.now() - started > 900) code = "ControlLeft+ShiftLeft";
    }
    if (token !== bindToken) return;
    if (code) {
      const saved = await saveSetting("hotkey", { mode: "pushToTalk", accelerator: code });
      if (saved) await refreshHotkey();
      await endBind();
      if (saved) toast(`Listen key set to ${state.hotkeyLabel}`);
      return;
    }
    if (Date.now() - started > BIND_TIMEOUT_MS) {
      await endBind();
      state.bindError = "No key detected. Click Change and hold a modifier or function key.";
      render();
      return;
    }
    window.setTimeout(() => void tick(), 60);
  };
  void tick();
}

async function endBind(): Promise<void> {
  if (!state.binding) return;
  bindToken++;
  state.binding = false;
  // The native watcher ignores the new key until it has been released, so the
  // keys still held from capture do not start a dictation.
  await call("suspend_hotkey", { paused: false }).catch(() => {});
  render();
}

// --- Updates -----------------------------------------------------------------

function applyUpdate(info: UpdateInfo): void {
  state.update.info = info;
  state.update.error = null;
  if (state.update.phase !== "installing") {
    state.update.phase = info.available ? "ready" : "latest";
  }
}

async function checkForUpdate(): Promise<void> {
  if (state.preview) return;
  state.update.phase = "checking";
  render();
  try {
    applyUpdate(await call<UpdateInfo>("check_for_update"));
  } catch (error) {
    state.update.phase = "error";
    state.update.error = errorText(error, "Could not check for updates.");
  }
  render();
}

async function installUpdate(): Promise<void> {
  state.update.phase = "installing";
  render();
  if (state.preview) return;
  try {
    // On success the app restarts before this resolves.
    const message = await call<string>("install_update");
    state.update.phase = "latest";
    state.update.info = { ...state.update.info, available: false, availableVersion: null };
    if (message) toast(message);
  } catch (error) {
    state.update.phase = "error";
    state.update.error = errorText(error, "The update did not install.");
  }
  render();
}

// --- Toast -------------------------------------------------------------------

function toast(message: string, tone: "info" | "error" = "info"): void {
  const el = root.querySelector<HTMLElement>("#toast");
  if (!el) return;
  el.textContent = message;
  el.dataset.tone = tone;
  el.classList.add("is-visible");
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("is-visible"), 2600);
}

// --- Formatting --------------------------------------------------------------

/**
 * D1 stores `datetime('now')`: UTC, space-separated, no zone. `new Date()`
 * reads that as local time, which shifts every timestamp by the UTC offset.
 */
export function parseServerDate(value: string): Date {
  const iso = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}(:\d{2}(\.\d+)?)?$/.test(value)
    ? `${value.replace(" ", "T")}Z`
    : value;
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? new Date() : date;
}

function dayLabel(date: Date): string {
  const today = new Date();
  const startOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((startOf(today) - startOf(date)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return date.toLocaleDateString(undefined, { weekday: "long" });
  return date.toLocaleDateString(undefined, {
    month: "long",
    day: "numeric",
    year: date.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

function formatTime(date: Date): string {
  return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

function averageWpm(rows: Transcript[]): number | null {
  let words = 0;
  let ms = 0;
  for (const row of rows) {
    // Very short clips are mostly key-press latency and skew the average.
    if (row.durationMs < 1_500) continue;
    words += (row.formatted || row.raw).split(/\s+/).filter(Boolean).length;
    ms += row.durationMs;
  }
  if (words < 20 || ms === 0) return null;
  return Math.round(words / (ms / 60_000));
}

function formatNumber(n: number): string {
  return new Intl.NumberFormat().format(n);
}

function initial(email: string | null): string {
  return (email?.trim()[0] ?? "W").toUpperCase();
}

function fallbackKeyLabel(accelerator: string): string {
  return accelerator
    .split("+")
    .filter(Boolean)
    .map((code) =>
      code
        .replace(/^(Control|Alt|Shift|Meta)(Left|Right)$/, (_, k: string, side: string) => {
          const name = ({ Control: "Ctrl", Alt: "Alt", Shift: "Shift", Meta: "Win" } as const)[
            k as "Control" | "Alt" | "Shift" | "Meta"
          ];
          return `${side} ${name}`;
        })
        .replace(/^Key/, "")
        .replace(/^Digit/, ""),
    )
    .join(" + ");
}

function errorText(error: unknown, fallback: string): string {
  const text = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  return text.trim() || fallback;
}

function esc(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// --- Icons (Lucide, MIT) -----------------------------------------------------

const svg = (body: string) =>
  `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;

const ICONS = {
  home: svg(
    '<path d="M3 10.5 12 3l9 7.5V20a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z"/>',
  ),
  book: svg(
    '<path d="M2 4h6a4 4 0 0 1 4 4v13a3 3 0 0 0-3-3H2z"/><path d="M22 4h-6a4 4 0 0 0-4 4v13a3 3 0 0 1 3-3h7z"/>',
  ),
  zap: svg('<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>'),
  settings: svg(
    '<path d="M20 7h-9"/><path d="M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>',
  ),
  copy: svg(
    '<rect width="13" height="13" x="9" y="9" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>',
  ),
  trash: svg(
    '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  ),
  search: svg('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
  plus: svg('<path d="M12 5v14"/><path d="M5 12h14"/>'),
  arrow: svg('<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>'),
  external: svg(
    '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  ),
  chevron: svg('<path d="m6 9 6 6 6-6"/>'),
  mic: svg(
    '<rect x="9" y="2" width="6" height="12" rx="3"/><path d="M19 10v1a7 7 0 0 1-14 0v-1"/><path d="M12 18v4"/>',
  ),
  history: svg(
    '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l3 2"/>',
  ),
  lock: svg(
    '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
  ),
};
