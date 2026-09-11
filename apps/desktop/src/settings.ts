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

const isMac = navigator.userAgent.includes("Mac");

const HOTKEYS = isMac
  ? [
      { value: "AltRight", label: "Right Option" },
      { value: "ControlRight", label: "Right Control" },
      { value: "F13", label: "F13" },
      { value: "F8", label: "F8" },
    ]
  : [
      { value: "ControlRight", label: "Right Ctrl" },
      { value: "AltRight", label: "Right Alt" },
      { value: "F8", label: "F8" },
      { value: "F13", label: "F13" },
    ];

export async function mountSettings(root: HTMLElement): Promise<void> {
  const [settings, status, version] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<Status>("get_status"),
    getVersion(),
  ]);

  const keyOptions = HOTKEYS.some((key) => key.value === settings.hotkey.accelerator)
    ? HOTKEYS
    : [...HOTKEYS, { value: settings.hotkey.accelerator, label: settings.hotkey.accelerator }];

  root.innerHTML = `
    <main class="settings">
      <header class="top">
        <div>
          <h1>WeldSpeak</h1>
          <p class="lede">Hold ${escapeHtml(labelFor(settings.hotkey.accelerator))} to talk.</p>
        </div>
        ${status.signedIn ? accountChip(status) : `<button id="sign-in" class="primary">Sign in</button>`}
      </header>

      ${status.canInject ? "" : accessibilityWarning()}
      ${status.signedIn ? "" : signedOutPanel()}
      ${status.signedIn ? dictionaryMarkup() : ""}

      <section class="group">
        <h2>Shortcut</h2>
        <label>
          <span>Hold to talk</span>
          <select id="accelerator">
            ${keyOptions
              .map(
                (key) =>
                  `<option value="${escapeHtml(key.value)}"${
                    key.value === settings.hotkey.accelerator ? " selected" : ""
                  }>${escapeHtml(key.label)}</option>`,
              )
              .join("")}
          </select>
        </label>
        <p class="hint" id="hotkey-hint"></p>
      </section>

      <section class="group">
        <h2>Dictation</h2>
        <label class="check">
          <input type="checkbox" id="cleanup" ${settings.cleanUpText ? "checked" : ""} />
          <span>
            Clean up speech
            <small>Drop filler and fix punctuation.</small>
          </span>
        </label>
        <label>
          <span>Insert by</span>
          <select id="injection">
            <option value="automatic">Automatic</option>
            <option value="alwaysType">Typing</option>
            <option value="alwaysPaste">Pasting</option>
          </select>
        </label>
      </section>

      <p class="build">WeldSpeak ${escapeHtml(version)} · updates itself</p>
    </main>
  `;

  const accelerator = root.querySelector<HTMLSelectElement>("#accelerator")!;
  const hint = root.querySelector<HTMLElement>("#hotkey-hint")!;
  const injection = root.querySelector<HTMLSelectElement>("#injection")!;
  injection.value = settings.injection;

  const save = async (patch: Partial<Settings>) => {
    await invoke("update_settings", { patch });
  };

  const validateHotkey = async () => {
    const error = await invoke<string | null>("validate_hotkey", {
      accelerator: accelerator.value,
    });
    hint.textContent = error ?? "";
    hint.classList.toggle("error", Boolean(error));
    return !error;
  };

  accelerator.addEventListener("change", async () => {
    if (await validateHotkey()) {
      await save({
        hotkey: { mode: "pushToTalk", accelerator: accelerator.value },
      });
      const lede = root.querySelector(".lede");
      if (lede) lede.textContent = `Hold ${labelFor(accelerator.value)} to talk.`;
    }
  });

  injection.addEventListener("change", () =>
    save({ injection: injection.value as Settings["injection"] }),
  );

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
  }

  await validateHotkey();
}

function labelFor(accelerator: string): string {
  return HOTKEYS.find((key) => key.value === accelerator)?.label ?? accelerator;
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
    <section class="group warning">
      <h2>Keyboard access needed</h2>
      <p>macOS has to allow WeldSpeak to type into other apps.</p>
      <button id="grant" class="primary">Open Accessibility</button>
    </section>
  `;
}

function signedOutPanel(): string {
  return `
    <section class="group">
      <h2>Account</h2>
      <p class="muted">Sign in with WeldSuite. A short code in the browser confirms this computer.</p>
      <p class="hint error" id="sign-in-error"></p>
      <p class="sign-in-code" id="sign-in-code" hidden>
        Confirm this code: <strong></strong>
      </p>
    </section>
  `;
}

function dictionaryMarkup(): string {
  return `
    <section class="group dictionary">
      <h2>Dictionary</h2>
      <p class="muted">Names, alloys and jargon the mic should not guess at.</p>
      <form id="term-form" class="term-form">
        <input id="term-input" type="text" maxlength="128" placeholder="Add a word or phrase" autocomplete="off" />
        <input id="sounds-input" type="text" maxlength="128" placeholder="Sounds like (optional)" autocomplete="off" />
        <button class="primary" type="submit">Add</button>
      </form>
      <p class="hint error" id="term-error"></p>
      <ul id="term-list" class="term-list"></ul>
    </section>
  `;
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

function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}
