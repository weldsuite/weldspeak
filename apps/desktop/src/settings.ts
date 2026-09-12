/**
 * The settings window.
 *
 * Held to the choices people actually make: sign in, the hold key, and the
 * words the mic should not guess. Everything else stays out of the way.
 */

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";

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

interface Microphone {
  name: string;
  isDefault: boolean;
}

interface Status {
  signedIn: boolean;
  email: string | null;
  orgs: Array<{ orgId: string; name: string; role: string }>;
  canInject: boolean;
}

interface DictionaryTerm {
  id: string;
  scope: string;
  term: string;
  soundsLike: string | null;
}

interface TranscriptRecord {
  id: string;
  formatted: string;
  createdAt: string;
}

const LOCALES = [
  { value: "", label: "Auto" },
  { value: "en", label: "English" },
  { value: "nl", label: "Dutch" },
  { value: "de", label: "German" },
  { value: "fr", label: "French" },
  { value: "es", label: "Spanish" },
  { value: "pt", label: "Portuguese" },
  { value: "it", label: "Italian" },
  { value: "pl", label: "Polish" },
  { value: "sv", label: "Swedish" },
  { value: "da", label: "Danish" },
  { value: "nb", label: "Norwegian" },
  { value: "fi", label: "Finnish" },
  { value: "tr", label: "Turkish" },
  { value: "ja", label: "Japanese" },
  { value: "ko", label: "Korean" },
  { value: "zh", label: "Chinese" },
  { value: "ar", label: "Arabic" },
  { value: "hi", label: "Hindi" },
];

let bindKeyListener: ((event: KeyboardEvent) => void) | null = null;

export async function mountSettings(root: HTMLElement): Promise<void> {
  if (bindKeyListener) {
    window.removeEventListener("keydown", bindKeyListener, true);
    window.removeEventListener("keyup", bindKeyListener, true);
    bindKeyListener = null;
  }

  const [settings, status, version, microphones] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<Status>("get_status"),
    getVersion(),
    invoke<Microphone[]>("list_microphones").catch(() => [] as Microphone[]),
  ]);

  const keyLabel = await invoke<string>("hotkey_label", {
    accelerator: settings.hotkey.accelerator,
  });

  root.innerHTML = `
    <main class="settings">
      <header class="top" data-tauri-drag-region>
        <div>
          <h1>WeldSpeak</h1>
          <p class="lede">Hold ${escapeHtml(keyLabel)} to talk</p>
        </div>
        ${status.signedIn ? accountChip(status) : ""}
      </header>

      ${status.canInject ? "" : accessibilityWarning()}
      ${status.signedIn ? "" : signedOutPanel()}

      <section class="block">
        <h2>Dictation</h2>
        <div class="card">
          <div class="row">
            <span class="row-copy">
              Hold to talk
              <small>Click, then press one key or two together. Double-tap for hands-free. Esc cancels.</small>
            </span>
            <button id="bind-key" class="bind-key" type="button">${escapeHtml(keyLabel)}</button>
          </div>
          <p class="hint" id="hotkey-hint"></p>
          <label class="row">
            <span class="row-copy">
              Microphone
              <small>The input WeldSpeak listens on</small>
            </span>
            <select id="microphone">
              ${microphoneOptions(microphones, settings.microphone ?? null)}
            </select>
          </label>
          <label class="row">
            <span class="row-copy">
              Clean up speech
              <small>Drop filler and fix punctuation</small>
            </span>
            <input id="cleanup" class="switch" type="checkbox" ${settings.cleanUpText ? "checked" : ""} />
          </label>
          <label class="row">
            <span class="row-copy">
              Pause media
              <small>Silence music and videos while you talk</small>
            </span>
            <input id="pause-media" class="switch" type="checkbox" ${settings.pauseMedia ? "checked" : ""} />
          </label>
          <label class="row">
            <span>Language</span>
            <select id="locale">
              ${LOCALES.map(
                (locale) =>
                  `<option value="${escapeHtml(locale.value)}"${
                    (settings.locale ?? "") === locale.value ? " selected" : ""
                  }>${escapeHtml(locale.label)}</option>`,
              ).join("")}
            </select>
          </label>
          <label class="row">
            <span>Insert by</span>
            <select id="injection">
              <option value="automatic">Automatic</option>
              <option value="alwaysType">Typing</option>
              <option value="alwaysPaste">Pasting</option>
            </select>
          </label>
        </div>
      </section>

      <section class="block">
        <h2>Snippets</h2>
        <div class="card">
          <p class="muted">Say the cue, get the saved text. “my email” can become your address.</p>
          <form id="snippet-form" class="snippet-form">
            <input id="snippet-trigger" type="text" maxlength="60" placeholder="Cue, e.g. my address" autocomplete="off" />
            <input id="snippet-expansion" type="text" maxlength="4000" placeholder="Text to insert" autocomplete="off" />
            <button class="primary" type="submit">Add</button>
          </form>
          <p class="hint error" id="snippet-error"></p>
          <ul id="snippet-list" class="term-list"></ul>
        </div>
      </section>

      ${status.signedIn ? dictionaryMarkup() : ""}
      ${status.signedIn ? historyMarkup() : ""}

      <section class="block">
        <h2>App</h2>
        <div class="card">
          <div class="row">
            <span class="row-copy">
              Version
              <small>WeldSpeak ${escapeHtml(version)} · ${settings.wordsDictated.toLocaleString()} words</small>
            </span>
            <button id="check-update" type="button">Update</button>
          </div>
          <p class="hint" id="update-hint"></p>
        </div>
      </section>
    </main>
  `;

  let hint = root.querySelector<HTMLElement>("#hotkey-hint")!;
  const injection = root.querySelector<HTMLSelectElement>("#injection")!;
  injection.value = settings.injection;
  let currentKey = settings.hotkey.accelerator;
  let capturing = false;
  const held = new Set<string>();
  let peak: string[] = [];

  const save = async (patch: Partial<Settings>) => {
    await invoke("update_settings", { patch });
  };

  const showHotkeyHint = async () => {
    const error = await invoke<string | null>("validate_hotkey", {
      accelerator: currentKey,
    });
    const warning = error
      ? null
      : await invoke<string | null>("hotkey_warning", { accelerator: currentKey });
    hint.textContent =
      error ?? warning ?? "Hold to talk. Double-tap for hands-free. Esc cancels.";
    hint.classList.toggle("error", Boolean(error));
    return !error;
  };

  const applyKey = async (accelerator: string) => {
    const error = await invoke<string | null>("validate_hotkey", { accelerator });
    if (error) {
      hint.textContent = error;
      hint.classList.add("error");
      return;
    }
    currentKey = accelerator;
    await save({ hotkey: { mode: "pushToTalk", accelerator } });
    const label = await invoke<string>("hotkey_label", { accelerator });
    const button = root.querySelector<HTMLButtonElement>("#bind-key");
    if (button) button.textContent = label;
    const lede = root.querySelector(".lede");
    if (lede) lede.textContent = `Hold ${label} to talk`;
    await showHotkeyHint();
  };

  const stopCapture = async () => {
    capturing = false;
    held.clear();
    peak = [];
    await invoke("suspend_hotkey", { paused: false });
    const button = root.querySelector<HTMLButtonElement>("#bind-key");
    if (button) {
      button.dataset.listening = "false";
      button.textContent = await invoke<string>("hotkey_label", { accelerator: currentKey });
    }
  };

  const startCapture = async () => {
    capturing = true;
    held.clear();
    peak = [];
    await invoke("suspend_hotkey", { paused: true });
    const button = root.querySelector<HTMLButtonElement>("#bind-key");
    if (button) {
      button.dataset.listening = "true";
      button.textContent = "Press a key…";
    }
    hint.textContent = "Press one key, or two together. Esc cancels.";
    hint.classList.remove("error");
  };

  const showPeak = async () => {
    const button = root.querySelector<HTMLButtonElement>("#bind-key");
    if (!button) return;
    if (peak.length === 0) {
      button.textContent = "Press a key…";
      return;
    }
    button.textContent = await invoke<string>("hotkey_label", {
      accelerator: peak.join("+"),
    });
  };

  root.querySelector("#bind-key")?.addEventListener("click", () => {
    void (capturing ? stopCapture() : startCapture());
  });

  bindKeyListener = (event: KeyboardEvent) => {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.code === "Escape") {
      held.clear();
      peak = [];
      void stopCapture();
      return;
    }
    if (event.repeat) return;
    if (event.type === "keydown") {
      if (held.size === 0) peak = [];
      held.add(event.code);
      if (!peak.includes(event.code) && peak.length < 2) peak.push(event.code);
      void showPeak();
      return;
    }
    if (event.type === "keyup") {
      held.delete(event.code);
      if (held.size === 0 && peak.length > 0) {
        const accelerator = peak.join("+");
        peak = [];
        void applyKey(accelerator).then(() => stopCapture());
      }
    }
  };
  window.addEventListener("keydown", bindKeyListener, true);
  window.addEventListener("keyup", bindKeyListener, true);

  injection.addEventListener("change", () =>
    save({ injection: injection.value as Settings["injection"] }),
  );

  root.querySelector("#microphone")!.addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    void save({ microphone: value ? value : null });
  });

  root.querySelector("#pause-media")!.addEventListener("change", (event) =>
    save({ pauseMedia: (event.target as HTMLInputElement).checked }),
  );

  root.querySelector("#locale")!.addEventListener("change", (event) => {
    const value = (event.target as HTMLSelectElement).value;
    void save({ locale: value ? value : null });
  });

  root.querySelector("#cleanup")!.addEventListener("change", (event) =>
    save({ cleanUpText: (event.target as HTMLInputElement).checked }),
  );

  root.querySelector("#grant")?.addEventListener("click", () =>
    invoke("open_permission_settings"),
  );

  root.querySelector("#sign-out")?.addEventListener("click", async () => {
    await invoke("sign_out");
    await mountSettings(root);
  });

  bindSignIn(root);

  if (status.signedIn) {
    await bindDictionary(root);
    await bindHistory(root);
  }

  bindSnippets(root, settings.snippets ?? [], save);
  bindUpdate(root);

  await showHotkeyHint();
}

function historyMarkup(): string {
  return `
    <section class="block">
      <h2>History</h2>
      <div class="card">
        <ul id="history-list" class="history-list"></ul>
      </div>
    </section>
  `;
}

function bindSnippets(
  root: HTMLElement,
  initial: Array<{ trigger: string; expansion: string }>,
  save: (patch: Partial<Settings>) => Promise<void>,
): void {
  let snippets = [...initial];
  const list = root.querySelector<HTMLUListElement>("#snippet-list");
  const form = root.querySelector<HTMLFormElement>("#snippet-form");
  const trigger = root.querySelector<HTMLInputElement>("#snippet-trigger");
  const expansion = root.querySelector<HTMLInputElement>("#snippet-expansion");
  const errorEl = root.querySelector<HTMLElement>("#snippet-error");
  if (!list || !form || !trigger || !expansion) return;

  const render = () => {
    if (snippets.length === 0) {
      list.innerHTML = `<li class="empty">Nothing here yet.</li>`;
      return;
    }
    list.innerHTML = snippets
      .map(
        (snippet, index) => `
        <li>
          <div class="term">
            <span>${escapeHtml(snippet.trigger)}</span>
            <small>${escapeHtml(snippet.expansion)}</small>
          </div>
          <button type="button" class="ghost" data-index="${index}">Remove</button>
        </li>`,
      )
      .join("");
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const cue = trigger.value.trim();
    const text = expansion.value.trim();
    if (!cue || !text) return;
    if (errorEl) errorEl.textContent = "";
    snippets = [...snippets, { trigger: cue, expansion: text }];
    await save({ snippets });
    trigger.value = "";
    expansion.value = "";
    trigger.focus();
    render();
  });

  list.addEventListener("click", async (event) => {
    const index = Number((event.target as HTMLElement | null)?.closest("button")?.dataset.index);
    if (Number.isNaN(index)) return;
    snippets = snippets.filter((_, item) => item !== index);
    await save({ snippets });
    render();
  });

  render();
}

async function bindHistory(root: HTMLElement): Promise<void> {
  const list = root.querySelector<HTMLUListElement>("#history-list");
  if (!list) return;

  const render = (records: TranscriptRecord[]) => {
    if (records.length === 0) {
      list.innerHTML = `<li class="empty">No dictations saved yet.</li>`;
      return;
    }
    list.innerHTML = records
      .map(
        (record) => `
        <li>
          <div class="term">
            <span>${escapeHtml(record.formatted)}</span>
          </div>
          <button type="button" class="ghost" data-delete="${escapeHtml(record.id)}">Remove</button>
        </li>`,
      )
      .join("");
  };

  try {
    render(await invoke<TranscriptRecord[]>("list_transcripts"));
  } catch {
    list.innerHTML = `<li class="empty">Sign in to see history.</li>`;
  }

  list.addEventListener("click", async (event) => {
    const id = (event.target as HTMLElement | null)?.closest("button")?.dataset.delete;
    if (!id) return;
    try {
      await invoke("delete_transcript", { id });
      render(await invoke<TranscriptRecord[]>("list_transcripts"));
    } catch {
      /* keep the current list */
    }
  });
}

function accountChip(status: Status): string {
  const name = status.email ?? "Signed in";
  return `
    <div class="account">
      <span class="account-name" title="${escapeHtml(name)}">${escapeHtml(name)}</span>
      <button id="sign-out" class="ghost">Sign out</button>
    </div>
  `;
}

function accessibilityWarning(): string {
  return `
    <section class="block warning">
      <h2>Keyboard access</h2>
      <div class="card">
        <p>macOS has to allow WeldSpeak to type into other apps.</p>
        <div class="row actions">
          <button id="grant" class="primary">Open Accessibility</button>
        </div>
      </div>
    </section>
  `;
}

function signedOutPanel(): string {
  return `
    <section class="block">
      <h2>Account</h2>
      <div class="card">
        <p class="muted">Sign in with WeldSuite. A short code in the browser confirms this computer.</p>
        <div class="row actions">
          <button id="sign-in" class="primary">Sign in</button>
        </div>
        <p class="hint error" id="sign-in-error"></p>
        <p class="sign-in-code" id="sign-in-code" hidden>
          Confirm this code: <strong></strong>
        </p>
      </div>
    </section>
  `;
}

function dictionaryMarkup(): string {
  return `
    <section class="block dictionary">
      <h2>Dictionary</h2>
      <div class="card">
        <p class="muted">Names, alloys and jargon the mic should not guess at. WeldSpeak adds them when you correct a dictation, and when a name stands out in what you said.</p>
        <form id="term-form" class="term-form">
          <input id="term-input" type="text" maxlength="128" placeholder="Add a word or phrase" autocomplete="off" />
          <input id="sounds-input" type="text" maxlength="128" placeholder="Sounds like (optional)" autocomplete="off" />
          <button class="primary" type="submit">Add</button>
        </form>
        <p class="hint error" id="term-error"></p>
        <ul id="term-list" class="term-list"></ul>
      </div>
    </section>
  `;
}

function bindUpdate(root: HTMLElement): void {
  const button = root.querySelector<HTMLButtonElement>("#check-update");
  const hint = root.querySelector<HTMLElement>("#update-hint");
  if (!button) return;

  button.addEventListener("click", async () => {
    if (hint) {
      hint.textContent = "";
      hint.classList.remove("error");
    }
    button.disabled = true;
    button.textContent = "Checking…";
    try {
      const message = await invoke<string>("install_update");
      if (hint) hint.textContent = message;
    } catch (error) {
      if (hint) {
        hint.textContent =
          typeof error === "string"
            ? error
            : error instanceof Error
              ? error.message
              : "Could not check for an update.";
        hint.classList.add("error");
      }
    } finally {
      button.disabled = false;
      button.textContent = "Update";
    }
  });
}

function bindSignIn(root: HTMLElement): void {
  const button = root.querySelector<HTMLButtonElement>("#sign-in");
  if (!button) return;

  button.addEventListener("click", async () => {
    const errorEl = root.querySelector<HTMLElement>("#sign-in-error");
    const codeEl = root.querySelector<HTMLElement>("#sign-in-code");
    if (errorEl) errorEl.textContent = "";
    button.disabled = true;
    button.textContent = "Opening browser…";
    try {
      const started = await invoke<{ verifyUrl: string; userCode: string }>("begin_sign_in");
      button.textContent = "Waiting…";
      if (codeEl) {
        codeEl.hidden = false;
        const strong = codeEl.querySelector("strong");
        if (strong) strong.textContent = started.userCode;
      }
    } catch (error) {
      button.disabled = false;
      button.textContent = "Sign in";
      if (errorEl) {
        errorEl.textContent =
          typeof error === "string"
            ? error
            : error instanceof Error
              ? error.message
              : "Could not start sign-in.";
      }
    }
  });
}

async function bindDictionary(root: HTMLElement): Promise<void> {
  const list = root.querySelector<HTMLUListElement>("#term-list");
  const form = root.querySelector<HTMLFormElement>("#term-form");
  const input = root.querySelector<HTMLInputElement>("#term-input");
  const sounds = root.querySelector<HTMLInputElement>("#sounds-input");
  const errorEl = root.querySelector<HTMLElement>("#term-error");
  if (!list || !form || !input || !sounds) return;

  const render = (terms: DictionaryTerm[]) => {
    if (terms.length === 0) {
      list.innerHTML = `<li class="empty">Nothing here yet.</li>`;
      return;
    }
    list.innerHTML = terms
      .map(
        (term) => `
        <li>
          <div class="term">
            <span>${escapeHtml(term.term)}</span>
            ${term.soundsLike ? `<small>sounds like ${escapeHtml(term.soundsLike)}</small>` : ""}
          </div>
          <button type="button" class="ghost" data-delete="${escapeHtml(term.id)}" aria-label="Remove ${escapeHtml(term.term)}">Remove</button>
        </li>`,
      )
      .join("");
  };

  const reload = async () => {
    try {
      render(await invoke<DictionaryTerm[]>("list_dictionary"));
    } catch (error) {
      list.innerHTML = `<li class="empty">${escapeHtml(
        typeof error === "string" ? error : "Could not load your dictionary.",
      )}</li>`;
    }
  };

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const term = input.value.trim();
    if (!term) return;
    if (errorEl) errorEl.textContent = "";
    try {
      const soundsLike = sounds.value.trim();
      await invoke("add_dictionary_term", {
        term,
        ...(soundsLike ? { soundsLike } : {}),
      });
      input.value = "";
      sounds.value = "";
      input.focus();
      await reload();
    } catch (error) {
      if (errorEl) {
        errorEl.textContent = typeof error === "string" ? error : "Could not add that term.";
      }
    }
  });

  list.addEventListener("click", async (event) => {
    const target = event.target as HTMLElement | null;
    const id = target?.closest("button")?.dataset.delete;
    if (!id) return;
    try {
      await invoke("delete_dictionary_term", { id });
      await reload();
    } catch (error) {
      if (errorEl) {
        errorEl.textContent = typeof error === "string" ? error : "Could not remove that term.";
      }
    }
  });

  await reload();
}

function microphoneOptions(mics: Microphone[], selected: string | null): string {
  const known = new Set(mics.map((mic) => mic.name));
  const missing =
    selected && !known.has(selected)
      ? `<option value="${escapeHtml(selected)}" selected>${escapeHtml(selected)} (unplugged)</option>`
      : "";
  return `
    <option value=""${selected ? "" : " selected"}>System default</option>
    ${missing}
    ${mics
      .map((mic) => {
        const label = mic.isDefault ? `${mic.name} (default)` : mic.name;
        const isSelected = selected === mic.name;
        return `<option value="${escapeHtml(mic.name)}"${isSelected ? " selected" : ""}>${escapeHtml(label)}</option>`;
      })
      .join("")}
  `;
}

function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}
