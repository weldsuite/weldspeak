/**
 * The recording overlay.
 *
 * A small pill that appears while dictating and shows the words as they are
 * recognised. It exists for one reason: without feedback, people cannot tell
 * whether the app heard them, so they stop mid-sentence to check — which is
 * exactly what a dictation tool must not make them do.
 *
 * Partials are shown but never injected. They are revised as the recognizer
 * gets more context, and the injected text is always the final.
 */

import { listen } from "@tauri-apps/api/event";

type Phase = "idle" | "listening" | "thinking" | "notice";

export function mountOverlay(root: HTMLElement): void {
  root.innerHTML = `
    <div class="pill" data-phase="idle">
      <span class="wave" aria-hidden="true">
        <span></span><span></span><span></span><span></span>
      </span>
      <span class="text" role="status" aria-live="polite"></span>
    </div>
  `;

  const pill = root.querySelector<HTMLElement>(".pill")!;
  const text = root.querySelector<HTMLElement>(".text")!;

  const set = (phase: Phase, message: string) => {
    pill.dataset.phase = phase;
    text.textContent = message;
  };

  void listen<string>("weldspeak://listening", () => set("listening", "Listening…")).catch(() => {
    // Preview / non-Tauri: leave idle until manually exercised.
  });

  void listen<string>("weldspeak://partial", (event) => {
    // Only the tail fits, and the tail is what the user just said — the part
    // they are checking was heard correctly.
    const words = event.payload.split(/\s+/);
    set("listening", words.slice(-12).join(" "));
  }).catch(() => undefined);

  void listen("weldspeak://thinking", () => set("thinking", "Tidying up…")).catch(() => undefined);
  void listen("weldspeak://done", () => set("idle", "")).catch(() => undefined);
  void listen<string>("weldspeak://notice", (event) => set("notice", event.payload)).catch(() => undefined);

  // Browser/Vite preview: cycle a short demo so the pill can be visually reviewed.
  if (!("__TAURI_INTERNALS__" in window) && new URLSearchParams(location.search).has("demo")) {
    set("listening", "Listening…");
    window.setTimeout(() => set("listening", "hold the key and speak clearly"), 900);
    window.setTimeout(() => set("thinking", "Tidying up…"), 2400);
    window.setTimeout(() => set("idle", ""), 3600);
  }
}
