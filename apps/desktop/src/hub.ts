/**
 * WeldSpeak Hub — webview shell (Home, Dictionary, Snippets, Settings).
 *
 * Dictation, hotkeys, injection, and the listening pill stay native. This
 * window is the calm Wispr-style surface for history and preferences.
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

const LOCALES: Array<[string, string]> = [
  ["", "Auto"],
  ["en", "English"],
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

const previewSettings: Settings = {
  apiBase: "https://weldspeak.weldsuite.org",
  hotkey: { mode: "pushToTalk", accelerator: "ControlRight" },
  orgId: "org_preview",
  injection: "automatic",
  cleanUpText: true,
  locale: null,
  keepHistory: true,
  pauseMedia: true,
  microphone: null,
  snippets: [
    { trigger: "my address", expansion: "12 Harbour Lane, Rotterdam" },
    { trigger: "sign off", expansion: "Best regards,\nGert" },
  ],
  wordsDictated: 12840,
};

const previewStatus: Status = {
  signedIn: true,
  email: "gert@weldsuite.org",
  orgs: [{ orgId: "org_preview", name: "WeldSuite", role: "admin" }],
  canInject: true,
};

const previewTranscripts: Transcript[] = [
  {
    id: "t1",
    raw: "can you send the inconel quote to marina",
    formatted: "Can you send the Inconel quote to Marina?",
    durationMs: 4200,
    appName: "Slack",
    createdAt: new Date(Date.now() - 12 * 60_000).toISOString(),
  },
  {
    id: "t2",
    raw: "schedule the fit-up inspection for thursday morning",
    formatted: "Schedule the fit-up inspection for Thursday morning.",
    durationMs: 5100,
    appName: "Mail",
    createdAt: new Date(Date.now() - 2 * 3600_000).toISOString(),
  },
  {
    id: "t3",
    raw: "um the heat input on pass three was a bit high",
    formatted: "The heat input on pass three was a bit high.",
    durationMs: 3800,
    appName: null,
    createdAt: new Date(Date.now() - 86400_000).toISOString(),
  },
];

const previewDictionary: DictionaryTerm[] = [
  {
    id: "d1",
    scope: "org",
    term: "Inconel 625",
    soundsLike: "in-co-nel",
    createdAt: new Date().toISOString(),
  },
  {
    id: "d2",
    scope: "user",
    term: "WeldSuite",
    soundsLike: null,
    createdAt: new Date().toISOString(),
  },
];

const previewMics: Microphone[] = [
  { name: "MacBook Pro Microphone", isDefault: true },
  { name: "USB Headset", isDefault: false },
];

let page: Page = "home";
let settings = previewSettings;
let status = previewStatus;
let transcripts = previewTranscripts;
let dictionary = previewDictionary;
let microphones = previewMics;
let hubRoot: HTMLElement | null = null;
let binding = false;
let previewMode = false;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

export async function mountHub(root: HTMLElement): Promise<void> {
  hubRoot = root;
  await refreshAll();
  render();
  if (!previewMode) {
    void listen("weldspeak://signed-in", async () => {
      await refreshAll();
      render();
    });
    void listen("weldspeak://history-changed", async () => {
      await refreshTranscripts();
      if (page === "home") render();
    });
  }
}

async function refreshAll(): Promise<void> {
  try {
    settings = await call<Settings>("get_settings");
    previewMode = false;
  } catch {
    previewMode = true;
    settings = previewSettings;
    status = previewStatus;
    transcripts = previewTranscripts;
    dictionary = previewDictionary;
    microphones = previewMics;
    return;
  }
  try {
    status = await call<Status>("get_status");
  } catch {
    status = { signedIn: false, email: null, orgs: [], canInject: true };
  }
  await Promise.all([refreshTranscripts(), refreshDictionary(), refreshMics()]);
}

async function refreshTranscripts(): Promise<void> {
  if (previewMode) {
    transcripts = previewTranscripts;
    return;
  }
  if (!status.signedIn) {
    transcripts = [];
    return;
  }
  try {
    transcripts = await call<Transcript[]>("list_transcripts");
  } catch {
    transcripts = [];
  }
}

async function refreshDictionary(): Promise<void> {
  if (previewMode) {
    dictionary = previewDictionary;
    return;
  }
  if (!status.signedIn) {
    dictionary = [];
    return;
  }
  try {
    dictionary = await call<DictionaryTerm[]>("list_dictionary");
  } catch {
    dictionary = [];
  }
}

async function refreshMics(): Promise<void> {
  if (previewMode) {
    microphones = previewMics;
    return;
  }
  try {
    microphones = await call<Microphone[]>("list_microphones");
  } catch {
    microphones = [];
  }
}

function render(): void {
  if (!hubRoot) return;
  hubRoot.innerHTML = `
    <div class="hub">
      <aside class="rail" aria-label="WeldSpeak">
        <div class="rail-brand">
          <span class="brand-orb" aria-hidden="true"></span>
          <div class="rail-brand-text">
            <strong>WeldSpeak</strong>
            <span>Hold · speak · go</span>
          </div>
        </div>
        <nav class="rail-nav">
          ${navButton("home", "Home")}
          ${navButton("dictionary", "Dictionary")}
          ${navButton("snippets", "Snippets")}
          ${navButton("settings", "Settings")}
        </nav>
        <div class="rail-foot">
          <span>${escapeHtml(formatWords(settings.wordsDictated))} words</span>
        </div>
      </aside>
      <main class="stage" data-page="${page}">
        ${pageContent()}
      </main>
    </div>
  `;
  wireChrome();
}

function navButton(id: Page, label: string): string {
  return `
    <button type="button" class="nav-item${page === id ? " is-active" : ""}" data-nav="${id}">
      <span class="nav-dot" aria-hidden="true"></span>
      ${label}
    </button>
  `;
}

function pageContent(): string {
  switch (page) {
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

function homePage(): string {
  const key = settings.hotkey.accelerator || "your key";
  const statusLine = status.signedIn
    ? `${formatWords(settings.wordsDictated)} words · Hold ${escapeHtml(prettyKey(key))} to talk`
    : `Hold ${escapeHtml(prettyKey(key))} to talk · Sign in to sync history`;

  return `
    <header class="stage-head">
      <div>
        <h1>Home</h1>
        <p class="stage-sub">${statusLine}</p>
      </div>
      ${
        status.signedIn
          ? ""
          : `<button type="button" class="primary" data-action="sign-in">Sign in</button>`
      }
    </header>
    <section class="history-sheet" aria-label="Recent dictations">
      ${
        !status.signedIn
          ? `<div class="empty">
              <p>Sign in to keep recent dictations across devices.</p>
              <button type="button" class="primary" data-action="sign-in">Sign in</button>
            </div>`
          : transcripts.length === 0
            ? `<div class="empty"><p>No dictations yet. Hold your key and speak.</p></div>`
            : `<ul class="history-list">
                ${transcripts.map((row) => historyRow(row)).join("")}
              </ul>`
      }
    </section>
  `;
}

function historyRow(row: Transcript): string {
  const when = formatWhen(row.createdAt);
  const meta = [row.appName, when].filter(Boolean).join(" · ");
  return `
    <li class="history-row" data-id="${escapeAttr(row.id)}">
      <div class="history-copy">
        <p class="history-text">${escapeHtml(row.formatted || row.raw)}</p>
        <span class="history-meta">${escapeHtml(meta)}</span>
      </div>
      <div class="history-actions">
        <button type="button" class="ghost" data-copy="${escapeAttr(row.formatted || row.raw)}">Copy</button>
        <button type="button" class="ghost danger" data-delete="${escapeAttr(row.id)}">Delete</button>
      </div>
    </li>
  `;
}

function dictionaryPage(): string {
  return `
    <header class="stage-head">
      <div>
        <h1>Dictionary</h1>
        <p class="stage-sub">Names and terms the recognizer should get right.</p>
      </div>
    </header>
    ${
      !status.signedIn
        ? signedOutGate("Sign in to manage your glossary.")
        : `
      <form class="composer" id="dict-form">
        <input name="term" placeholder="Term" required autocomplete="off" />
        <input name="sounds" placeholder="Sounds like (optional)" autocomplete="off" />
        <button type="submit" class="primary">Add</button>
      </form>
      <section class="list-sheet">
        ${
          dictionary.length === 0
            ? `<div class="empty"><p>No terms yet.</p></div>`
            : `<ul class="plain-list">
                ${dictionary
                  .map(
                    (term) => `
                  <li>
                    <div>
                      <strong>${escapeHtml(term.term)}</strong>
                      ${
                        term.soundsLike
                          ? `<span class="muted"> · ${escapeHtml(term.soundsLike)}</span>`
                          : ""
                      }
                      <div class="tiny muted">${escapeHtml(term.scope)}</div>
                    </div>
                    <button type="button" class="ghost danger" data-del-term="${escapeAttr(term.id)}">Delete</button>
                  </li>`,
                  )
                  .join("")}
              </ul>`
        }
      </section>`
    }
  `;
}

function snippetsPage(): string {
  const snippets = settings.snippets ?? [];
  return `
    <header class="stage-head">
      <div>
        <h1>Snippets</h1>
        <p class="stage-sub">Say a cue; WeldSpeak inserts the saved text.</p>
      </div>
    </header>
    <form class="composer" id="snip-form">
      <input name="trigger" placeholder="Cue, e.g. my address" required autocomplete="off" />
      <input name="expansion" placeholder="Text to insert" required autocomplete="off" />
      <button type="submit" class="primary">Add</button>
    </form>
    <section class="list-sheet">
      ${
        snippets.length === 0
          ? `<div class="empty"><p>No snippets yet.</p></div>`
          : `<ul class="plain-list">
              ${snippets
                .map(
                  (snip, index) => `
                <li>
                  <div>
                    <strong>${escapeHtml(snip.trigger)}</strong>
                    <div class="muted snip-exp">${escapeHtml(snip.expansion)}</div>
                  </div>
                  <button type="button" class="ghost danger" data-del-snip="${index}">Delete</button>
                </li>`,
                )
                .join("")}
            </ul>`
      }
    </section>
  `;
}

function settingsPage(): string {
  const hotkeyLabel = prettyKey(settings.hotkey.accelerator);
  const micOptions = [
    `<option value="">System default</option>`,
    ...microphones.map(
      (mic) =>
        `<option value="${escapeAttr(mic.name)}"${
          settings.microphone === mic.name ? " selected" : ""
        }>${escapeHtml(mic.name)}${mic.isDefault ? " (default)" : ""}</option>`,
    ),
  ].join("");

  const localeOptions = LOCALES.map(
    ([value, label]) =>
      `<option value="${escapeAttr(value)}"${
        (settings.locale ?? "") === value ? " selected" : ""
      }>${escapeHtml(label)}</option>`,
  ).join("");

  const orgOptions =
    status.orgs.length === 0
      ? `<option value="">Personal</option>`
      : status.orgs
          .map(
            (org) =>
              `<option value="${escapeAttr(org.orgId)}"${
                settings.orgId === org.orgId ? " selected" : ""
              }>${escapeHtml(org.name)}</option>`,
          )
          .join("");

  return `
    <header class="stage-head">
      <div>
        <h1>Settings</h1>
        <p class="stage-sub">Account, dictation key, and how text is inserted.</p>
      </div>
    </header>
    <div class="settings-stack">
      ${status.canInject ? "" : accessibilityWarning()}
      <section class="panel">
        <h2 class="panel-title">Account</h2>
        <div class="panel-body">
          ${
            status.signedIn
              ? `<p class="account-email">${escapeHtml(status.email ?? "Signed in")}</p>
                 <p class="muted account-orgs">${escapeHtml(
                   status.orgs.map((o) => o.name).join(", ") || "Personal",
                 )}</p>
                 <div class="panel-actions">
                   <button type="button" class="ghost" data-action="sign-out">Sign out</button>
                   <button type="button" class="ghost" data-action="dashboard">Open dashboard</button>
                 </div>`
              : `<p class="muted" style="margin:0 0 12px">Sign in opens your browser, then shows a short code to confirm.</p>
                 <div class="panel-actions">
                   <button type="button" class="primary" data-action="sign-in">Sign in</button>
                 </div>`
          }
        </div>
      </section>

      <section class="panel">
        <h2 class="panel-title">Dictation</h2>
        <div class="panel-body">
          <div class="row">
            <span class="row-label"><span>Key</span><small>Hold to talk</small></span>
            <div class="bind-group">
              <button type="button" class="primary" id="bind-key">${
                binding ? "Listening…" : escapeHtml(hotkeyLabel)
              }</button>
            </div>
          </div>
          <p class="hint" id="hotkey-hint"></p>
          <label class="row">
            <span class="row-label"><span>Microphone</span></span>
            <select id="microphone">${micOptions}</select>
          </label>
          <label class="row">
            <span class="row-label"><span>Language</span></span>
            <select id="locale">${localeOptions}</select>
          </label>
          ${
            status.orgs.length > 0
              ? `<label class="row">
                   <span class="row-label"><span>Organization</span></span>
                   <select id="org">${orgOptions}</select>
                 </label>`
              : ""
          }
        </div>
      </section>

      <section class="panel">
        <h2 class="panel-title">Text</h2>
        <div class="panel-body">
          <label class="row check">
            <input type="checkbox" id="cleanup" ${settings.cleanUpText ? "checked" : ""} />
            <span class="row-label">
              <span>Clean up what I say</span>
              <small>Removes fillers, fixes punctuation. Off inserts the raw transcript.</small>
            </span>
          </label>
          <label class="row">
            <span class="row-label"><span>Insert by</span></span>
            <select id="injection">
              <option value="automatic">Choosing automatically</option>
              <option value="alwaysType">Typing</option>
              <option value="alwaysPaste">Pasting</option>
            </select>
          </label>
          <label class="row check">
            <input type="checkbox" id="pause-media" ${settings.pauseMedia ? "checked" : ""} />
            <span class="row-label">
              <span>Pause other audio while talking</span>
            </span>
          </label>
          <label class="row check">
            <input type="checkbox" id="keep-history" ${settings.keepHistory ? "checked" : ""} />
            <span class="row-label">
              <span>Keep my dictations</span>
              <small>Your team’s admin can turn this off for everyone.</small>
            </span>
          </label>
        </div>
      </section>

      <section class="panel">
        <h2 class="panel-title">Updates</h2>
        <div class="panel-body">
          <div class="panel-actions">
            <button type="button" class="ghost" data-action="check-update">Check for update</button>
          </div>
          <p class="tiny muted" style="margin:12px 0 0">WeldSpeak · ${formatWords(settings.wordsDictated)} words dictated</p>
        </div>
      </section>
    </div>
  `;
}

function accessibilityWarning(): string {
  return `
    <section class="panel warning">
      <h2 class="panel-title">Accessibility</h2>
      <div class="panel-body">
        <p>macOS needs Accessibility permission before WeldSpeak can type into other apps.</p>
        <div class="panel-actions">
          <button type="button" class="primary" data-action="grant">Open Accessibility settings</button>
        </div>
      </div>
    </section>
  `;
}

function signedOutGate(message: string): string {
  return `
    <div class="empty">
      <p>${escapeHtml(message)}</p>
      <button type="button" class="primary" data-action="sign-in">Sign in</button>
    </div>
  `;
}

function wireChrome(): void {
  hubRoot?.querySelectorAll<HTMLButtonElement>("[data-nav]").forEach((button) => {
    button.addEventListener("click", () => {
      page = button.dataset.nav as Page;
      render();
    });
  });

  hubRoot?.querySelectorAll<HTMLElement>("[data-action]").forEach((el) => {
    el.addEventListener("click", () => {
      void handleAction(el.dataset.action!);
    });
  });

  hubRoot?.querySelectorAll<HTMLButtonElement>("[data-copy]").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await call("copy_text", { text: button.dataset.copy });
      } catch {
        // preview
      }
    });
  });

  hubRoot?.querySelectorAll<HTMLButtonElement>("[data-delete]").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await call("delete_transcript", { id: button.dataset.delete });
        await refreshTranscripts();
        render();
      } catch {
        // preview
      }
    });
  });

  hubRoot?.querySelectorAll<HTMLButtonElement>("[data-del-term]").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await call("delete_dictionary_term", { id: button.dataset.delTerm });
        await refreshDictionary();
        render();
      } catch {
        // preview
      }
    });
  });

  hubRoot?.querySelectorAll<HTMLButtonElement>("[data-del-snip]").forEach((button) => {
    button.addEventListener("click", async () => {
      const index = Number(button.dataset.delSnip);
      const next = settings.snippets.filter((_, i) => i !== index);
      await save({ snippets: next });
      settings.snippets = next;
      render();
    });
  });

  const dictForm = hubRoot?.querySelector<HTMLFormElement>("#dict-form");
  dictForm?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(dictForm);
    const term = String(data.get("term") ?? "").trim();
    const sounds = String(data.get("sounds") ?? "").trim();
    try {
      await call("add_dictionary_term", {
        term,
        soundsLike: sounds || null,
      });
      await refreshDictionary();
      render();
    } catch (error) {
      window.alert(String(error));
    }
  });

  const snipForm = hubRoot?.querySelector<HTMLFormElement>("#snip-form");
  snipForm?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const data = new FormData(snipForm);
    const trigger = String(data.get("trigger") ?? "").trim();
    const expansion = String(data.get("expansion") ?? "").trim();
    if (!trigger || !expansion) return;
    const next = [
      ...settings.snippets.filter((s) => s.trigger !== trigger),
      { trigger, expansion },
    ];
    await save({ snippets: next });
    settings.snippets = next;
    render();
  });

  if (page === "settings") wireSettings();
}

function wireSettings(): void {
  const injection = hubRoot?.querySelector<HTMLSelectElement>("#injection");
  if (injection) injection.value = settings.injection;

  injection?.addEventListener("change", () =>
    save({ injection: injection.value as Settings["injection"] }),
  );

  hubRoot?.querySelector("#cleanup")?.addEventListener("change", (event) =>
    save({ cleanUpText: (event.target as HTMLInputElement).checked }),
  );
  hubRoot?.querySelector("#pause-media")?.addEventListener("change", (event) =>
    save({ pauseMedia: (event.target as HTMLInputElement).checked }),
  );
  hubRoot?.querySelector("#keep-history")?.addEventListener("change", (event) =>
    save({ keepHistory: (event.target as HTMLInputElement).checked }),
  );

  hubRoot?.querySelector("#microphone")?.addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    void save({ microphone: value || null });
  });

  hubRoot?.querySelector("#locale")?.addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    void save({ locale: value || null });
  });

  hubRoot?.querySelector("#org")?.addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    void save({ orgId: value || null });
  });

  hubRoot?.querySelector("#bind-key")?.addEventListener("click", () => {
    void startBind();
  });

  void refreshHotkeyHint();
}

async function handleAction(action: string): Promise<void> {
  switch (action) {
    case "sign-in":
      try {
        const started = await call<SignInStarted>("begin_sign_in");
        window.alert(`Confirm ${started.userCode} in your browser.`);
      } catch {
        // preview
      }
      break;
    case "sign-out":
      try {
        await call("sign_out");
        status = { ...status, signedIn: false, email: null, orgs: [] };
        transcripts = [];
        dictionary = [];
        render();
      } catch {
        // preview
      }
      break;
    case "grant":
      void call("open_permission_settings").catch(() => undefined);
      break;
    case "dashboard":
      void call("open_external_url", {
        url: "https://weldspeak.com/dictionary",
      }).catch(() => undefined);
      break;
    case "check-update":
      try {
        await call("install_update");
      } catch (error) {
        window.alert(String(error));
      }
      break;
  }
}

async function save(patch: Record<string, unknown>): Promise<void> {
  try {
    settings = await call<Settings>("update_settings", { patch });
  } catch {
    settings = { ...settings, ...patch } as Settings;
  }
}

async function startBind(): Promise<void> {
  if (binding) return;
  binding = true;
  render();
  try {
    await call("suspend_hotkey", { paused: true });
  } catch {
    // preview
  }

  const started = Date.now();
  const tick = async () => {
    try {
      const code = await call<string | null>("poll_held_hotkey");
      if (code) {
        await save({
          hotkey: { mode: "pushToTalk", accelerator: code },
        });
        await endBind();
        return;
      }
    } catch {
      // preview: fake after a moment
      if (Date.now() - started > 800) {
        await save({
          hotkey: { mode: "pushToTalk", accelerator: "ControlRight" },
        });
        await endBind();
        return;
      }
    }
    if (Date.now() - started > 12_000) {
      await endBind();
      return;
    }
    window.setTimeout(() => void tick(), 80);
  };
  void tick();
}

async function endBind(): Promise<void> {
  binding = false;
  try {
    await call("suspend_hotkey", { paused: false });
  } catch {
    // preview
  }
  render();
}

async function refreshHotkeyHint(): Promise<void> {
  const hint = hubRoot?.querySelector<HTMLElement>("#hotkey-hint");
  if (!hint) return;
  try {
    const warning = await call<string | null>("hotkey_warning", {
      accelerator: settings.hotkey.accelerator,
    });
    hint.textContent = warning ?? "";
    hint.classList.toggle("error", Boolean(warning));
  } catch {
    hint.textContent = "";
  }
}

function formatWords(n: number): string {
  return new Intl.NumberFormat().format(n);
}

function prettyKey(accelerator: string): string {
  const map: Record<string, string> = {
    ControlRight: "Right Ctrl",
    ControlLeft: "Left Ctrl",
    AltRight: "Right Alt",
    AltLeft: "Left Alt",
    ShiftRight: "Right Shift",
    ShiftLeft: "Left Shift",
    MetaRight: "Right ⌘",
    MetaLeft: "Left ⌘",
    CapsLock: "Caps Lock",
  };
  return map[accelerator] ?? accelerator;
}

function formatWhen(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const diff = Date.now() - date.getTime();
  if (diff < 60_000) return "Just now";
  if (diff < 3600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86400_000) return `${Math.floor(diff / 3600_000)}h ago`;
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}

function escapeAttr(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/"/g, "&quot;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
