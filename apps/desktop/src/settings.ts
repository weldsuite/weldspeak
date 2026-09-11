/**
 * The settings window.
 *
 * Everything here is a choice people genuinely differ on. Anything with a
 * defensible default is not a setting — a dictation tool that needs configuring
 * before it works has already lost.
 */

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
  /** False on macOS until the user grants Accessibility. */
  canInject: boolean;
}

export async function mountSettings(root: HTMLElement): Promise<void> {
  const [settings, status] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<Status>("get_status"),
  ]);

  root.innerHTML = `
    <main class="settings">
      <header>
        <h1>WeldSpeak</h1>
        <p class="muted">Hold your dictation key and speak. A bar appears while it is listening.</p>
      </header>

      ${status.canInject ? "" : accessibilityWarning()}
      ${status.signedIn ? signedInPanel(status) : signedOutPanel()}

      <section class="group">
        <h2>Dictation key</h2>
        <label>
          <span>Behaviour</span>
          <select id="mode">
            <option value="pushToTalk">Hold to talk</option>
            <option value="toggle">Press to start and stop</option>
          </select>
        </label>
        <label>
          <span>Key</span>
          <input id="accelerator" value="${escapeHtml(settings.hotkey.accelerator)}" />
        </label>
        <p class="hint" id="hotkey-hint"></p>
        <p class="hint">Hold the key — you should see a listening bar at the bottom of the screen.</p>
      </section>

      <section class="group">
        <h2>Text</h2>
        <label class="check">
          <input type="checkbox" id="cleanup" ${settings.cleanUpText ? "checked" : ""} />
          <span>
            Clean up what I say
            <small>Removes “um”, fixes punctuation and casing. Turn off to insert
            exactly what was heard.</small>
          </span>
        </label>
        <label>
          <span>Insert by</span>
          <select id="injection">
            <option value="automatic">Choosing automatically</option>
            <option value="alwaysType">Typing (never touches the clipboard)</option>
            <option value="alwaysPaste">Pasting (fastest for long text)</option>
          </select>
        </label>
      </section>

      <section class="group">
        <h2>History</h2>
        <label class="check">
          <input type="checkbox" id="history" ${settings.keepHistory ? "checked" : ""} />
          <span>
            Keep my dictations
            <small>Your team&rsquo;s admin can turn this off for everyone.</small>
          </span>
        </label>
      </section>
    </main>
  `;

  const mode = root.querySelector<HTMLSelectElement>("#mode")!;
  const accelerator = root.querySelector<HTMLInputElement>("#accelerator")!;
  const hint = root.querySelector<HTMLElement>("#hotkey-hint")!;
  const injection = root.querySelector<HTMLSelectElement>("#injection")!;

  mode.value = settings.hotkey.mode;
  injection.value = settings.injection;

  const save = async (patch: Partial<Settings>) => {
    await invoke("update_settings", { patch });
  };

  const validateHotkey = async () => {
    if (mode.value !== "pushToTalk") {
      hint.textContent = "";
      return true;
    }
    // The Rust side owns this rule — Fn in particular is not deliverable on
    // macOS — so the check happens there rather than being duplicated here.
    const error = await invoke<string | null>("validate_hotkey", {
      accelerator: accelerator.value,
    });
    hint.textContent = error ?? "";
    hint.classList.toggle("error", Boolean(error));
    return !error;
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

  root.querySelector("#grant")?.addEventListener("click", () =>
    invoke("open_permission_settings"),
  );

  root.querySelector("#sign-in")?.addEventListener("click", async () => {
    const button = root.querySelector<HTMLButtonElement>("#sign-in")!;
    const errorEl = root.querySelector<HTMLElement>("#sign-in-error")!;
    const codeEl = root.querySelector<HTMLElement>("#sign-in-code")!;
    errorEl.textContent = "";
    button.disabled = true;
    button.textContent = "Opening browser…";
    try {
      // The Rust side opens the system browser. Clerk needs a real origin;
      // the Tauri webview cannot host that session.
      const started = await invoke<{ verifyUrl: string; userCode: string }>("begin_sign_in");
      button.textContent = "Waiting for browser…";
      codeEl.hidden = false;
      codeEl.querySelector("strong")!.textContent = started.userCode;
    } catch (error) {
      button.disabled = false;
      button.textContent = "Sign in";
      errorEl.textContent =
        typeof error === "string"
          ? error
          : error instanceof Error
            ? error.message
            : "Could not start sign-in. Check your connection and try again.";
    }
  });

  root.querySelector("#sign-out")?.addEventListener("click", async () => {
    await invoke("sign_out");
    await mountSettings(root);
  });

  await validateHotkey();
}

function accessibilityWarning(): string {
  return `
    <section class="group warning">
      <h2>WeldSpeak cannot type yet</h2>
      <p>
        macOS needs to allow WeldSpeak to control your keyboard before it can put
        text into other apps. Until then, dictations are copied to your clipboard.
      </p>
      <button id="grant" class="primary">Open Accessibility settings</button>
    </section>
  `;
}

function signedInPanel(status: Status): string {
  return `
    <section class="group">
      <h2>Account</h2>
      <p>${escapeHtml(status.email ?? "Signed in")}</p>
      ${
        status.orgs.length > 0
          ? `<p class="muted">${escapeHtml(status.orgs.map((o) => o.name).join(", "))}</p>`
          : ""
      }
      <button id="sign-out">Sign out</button>
    </section>
  `;
}

function signedOutPanel(): string {
  return `
    <section class="group">
      <h2>Sign in</h2>
      <p class="muted">
        WeldSpeak opens your browser to sign in, then shows you a short code to
        confirm.
      </p>
      <button id="sign-in" class="primary">Sign in</button>
      <p class="hint error" id="sign-in-error"></p>
      <p class="sign-in-code" id="sign-in-code" hidden>
        Confirm this code in the browser: <strong></strong>
      </p>
    </section>
  `;
}

/** Escape text interpolated into markup. */
function escapeHtml(value: string): string {
  const element = document.createElement("span");
  element.textContent = value;
  return element.innerHTML;
}
