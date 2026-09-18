/**
 * The settings window.
 *
 * Everything here is a choice people genuinely differ on. Anything with a
 * defensible default is not a setting — a dictation tool that needs configuring
 * before it works has already lost.
 */

import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

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
  /** False on macOS until the user grants Accessibility. */
  canInject: boolean;
}

const previewSettings: Settings = {
  apiBase: "https://api.weldspeak.com",
  hotkey: { mode: "pushToTalk", accelerator: "Fn" },
  orgId: null,
  injection: "automatic",
  cleanUpText: true,
  locale: null,
  keepHistory: true,
};

const previewStatus: Status = {
  signedIn: true,
  email: "you@weldsuite.org",
  orgs: [{ orgId: "org_preview", name: "WeldSuite", role: "admin" }],
  canInject: true,
};

async function loadSettings(): Promise<Settings> {
  try {
    return await invoke<Settings>("get_settings");
  } catch {
    // Vite preview / browser demos have no Tauri bridge.
    return previewSettings;
  }
}

async function loadStatus(): Promise<Status> {
  try {
    return await invoke<Status>("get_status");
  } catch {
    return previewStatus;
  }
}

export async function mountSettings(root: HTMLElement): Promise<void> {
  const [settings, status] = await Promise.all([loadSettings(), loadStatus()]);

  root.innerHTML = `
    <main class="settings">
      <header class="settings-header">
        <div class="brand">
          <div class="brand-mark">
            <span class="brand-orb" aria-hidden="true"></span>
            <h1>WeldSpeak</h1>
          </div>
          <p class="brand-tagline">Hold your dictation key, speak, let go.</p>
        </div>
      </header>

      <div class="stack">
        ${status.canInject ? "" : accessibilityWarning()}
        ${status.signedIn ? signedInPanel(status) : signedOutPanel()}

        <section class="panel">
          <h2 class="panel-title">Dictation key</h2>
          <div class="panel-body">
            <label class="row">
              <span class="row-label"><span>Behaviour</span></span>
              <select id="mode">
                <option value="pushToTalk">Hold to talk</option>
                <option value="toggle">Press to start and stop</option>
              </select>
            </label>
            <label class="row">
              <span class="row-label"><span>Key</span></span>
              <input id="accelerator" value="${escapeHtml(settings.hotkey.accelerator)}" />
            </label>
            <p class="hint" id="hotkey-hint"></p>
          </div>
        </section>

        <section class="panel">
          <h2 class="panel-title">Text</h2>
          <div class="panel-body">
            <label class="row check">
              <input type="checkbox" id="cleanup" ${settings.cleanUpText ? "checked" : ""} />
              <span class="row-label">
                <span>Clean up what I say</span>
                <small>Removes “um”, fixes punctuation and casing. Turn off to insert
                exactly what was heard.</small>
              </span>
            </label>
            <label class="row">
              <span class="row-label"><span>Insert by</span></span>
              <select id="injection">
                <option value="automatic">Choosing automatically</option>
                <option value="alwaysType">Typing (never touches the clipboard)</option>
                <option value="alwaysPaste">Pasting (fastest for long text)</option>
              </select>
            </label>
          </div>
        </section>

        <section class="panel">
          <h2 class="panel-title">History</h2>
          <div class="panel-body">
            <label class="row check">
              <input type="checkbox" id="history" ${settings.keepHistory ? "checked" : ""} />
              <span class="row-label">
                <span>Keep my dictations</span>
                <small>Your team&rsquo;s admin can turn this off for everyone.</small>
              </span>
            </label>
          </div>
        </section>
      </div>
    </main>
  `;

  const mode = root.querySelector<HTMLSelectElement>("#mode")!;
  const accelerator = root.querySelector<HTMLInputElement>("#accelerator")!;
  const hint = root.querySelector<HTMLElement>("#hotkey-hint")!;
  const injection = root.querySelector<HTMLSelectElement>("#injection")!;

  mode.value = settings.hotkey.mode;
  injection.value = settings.injection;

  const save = async (patch: Partial<Settings>) => {
    try {
      await invoke("update_settings", { patch });
    } catch {
      // Preview mode: ignore persistence.
    }
  };

  const validateHotkey = async () => {
    if (mode.value !== "pushToTalk") {
      hint.textContent = "";
      return true;
    }
    // The Rust side owns this rule — Fn in particular is not deliverable on
    // macOS — so the check happens there rather than being duplicated here.
    try {
      const error = await invoke<string | null>("validate_hotkey", {
        accelerator: accelerator.value,
      });
      hint.textContent = error ?? "";
      hint.classList.toggle("error", Boolean(error));
      return !error;
    } catch {
      hint.textContent = "";
      return true;
    }
  };

  mode.addEventListener("change", async () => {
    if (await validateHotkey()) {
      await save({ hotkey: { mode: mode.value as Settings["hotkey"]["mode"], accelerator: accelerator.value } });
    }
  });

  accelerator.addEventListener("change", async () => {
    if (await validateHotkey()) {
      await save({ hotkey: { mode: mode.value as Settings["hotkey"]["mode"], accelerator: accelerator.value } });
    }
  });

  injection.addEventListener("change", () =>
    save({ injection: injection.value as Settings["injection"] }),
  );

  root.querySelector("#cleanup")!.addEventListener("change", (event) =>
    save({ cleanUpText: (event.target as HTMLInputElement).checked }),
  );

  root.querySelector("#history")!.addEventListener("change", (event) =>
    save({ keepHistory: (event.target as HTMLInputElement).checked }),
  );

  root.querySelector("#grant")?.addEventListener("click", () => {
    void invoke("open_permission_settings").catch(() => undefined);
  });

  root.querySelector("#sign-in")?.addEventListener("click", async () => {
    // Sign-in happens in the system browser: Clerk needs real cookies on a real
    // origin, which the Tauri webview cannot provide.
    try {
      const url = await invoke<string>("begin_sign_in");
      await openUrl(url);
    } catch {
      // Preview mode.
    }
  });

  root.querySelector("#sign-out")?.addEventListener("click", () => {
    void invoke("sign_out").catch(() => undefined);
  });

  await validateHotkey();
}

function accessibilityWarning(): string {
  return `
    <section class="panel warning">
      <h2 class="panel-title">Accessibility</h2>
      <div class="panel-body">
        <p>
          macOS needs to allow WeldSpeak to control your keyboard before it can put
          text into other apps. Until then, dictations are copied to your clipboard.
        </p>
        <div class="panel-actions">
          <button id="grant" class="primary">Open Accessibility settings</button>
        </div>
      </div>
    </section>
  `;
}

function signedInPanel(status: Status): string {
  return `
    <section class="panel">
      <h2 class="panel-title">Account</h2>
      <div class="panel-body">
        <p class="account-email">${escapeHtml(status.email ?? "Signed in")}</p>
        ${
          status.orgs.length > 0
            ? `<p class="account-orgs muted">${escapeHtml(status.orgs.map((o) => o.name).join(", "))}</p>`
            : ""
        }
        <div class="panel-actions">
          <button id="sign-out" class="ghost">Sign out</button>
        </div>
      </div>
    </section>
  `;
}

function signedOutPanel(): string {
  return `
    <section class="panel">
      <h2 class="panel-title">Sign in</h2>
      <div class="panel-body">
        <p class="muted" style="margin: 0 0 14px">
          WeldSpeak opens your browser to sign in, then shows you a short code to
          confirm.
        </p>
        <div class="panel-actions">
          <button id="sign-in" class="primary">Sign in</button>
        </div>
      </div>
    </section>
  `;
}

/** Escape text interpolated into markup. */
function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}
