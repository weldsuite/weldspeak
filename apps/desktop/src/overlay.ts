/**
 * The recording overlay.
 *
 * A small pill that appears while dictating and shows that the microphone is
 * live — without that, people stop mid-sentence to check, which is exactly
 * what a dictation tool must not make them do. Partials are shown but never
 * injected; the injected text is always the final.
 */

import { listen } from "@tauri-apps/api/event";

type Phase = "idle" | "listening" | "thinking" | "notice";

export function mountOverlay(root: HTMLElement): void {
  root.innerHTML = `
    <div class="pill" data-phase="idle">
      <div class="waveform" aria-hidden="true">${Array.from({ length: 5 }, () => "<span></span>").join("")}</div>
      <span class="text" role="status" aria-live="polite"></span>
    </div>
  `;

  const pill = root.querySelector<HTMLElement>(".pill")!;
  const text = root.querySelector<HTMLElement>(".text")!;
  const bars = [...root.querySelectorAll<HTMLElement>(".waveform span")];

  const set = (phase: Phase, message: string) => {
    pill.dataset.phase = phase;
    text.textContent = message;
    if (phase === "listening") {
      for (const bar of bars) {
        bar.style.animation = "";
        bar.style.transform = "";
      }
    } else {
      for (const bar of bars) {
        bar.style.animation = "none";
        bar.style.transform = "scaleY(0.2)";
      }
    }
  };

  void listen("weldspeak://listening", () => set("listening", ""));

  void listen<string>("weldspeak://partial", (event) => {
    const words = event.payload.trim().split(/\s+/).filter(Boolean);
    set("listening", words.slice(-6).join(" "));
  });

  void listen<number>("weldspeak://level", (event) => {
    if (pill.dataset.phase !== "listening") return;
    const boosted = Math.min(1, Math.max(0, event.payload) * 8);
    const now = Date.now();
    bars.forEach((bar, index) => {
      const centre = 1 - Math.abs(index - 2) / 3;
      const idle = 0.18 + 0.16 * Math.abs(Math.sin(now / 160 + index));
      const height = Math.max(idle, Math.min(1, boosted * (0.45 + centre)));
      bar.style.animation = "none";
      bar.style.transform = `scaleY(${height})`;
    });
  });

  void listen("weldspeak://thinking", () => set("thinking", ""));
  void listen("weldspeak://done", () => set("idle", ""));
  void listen<string>("weldspeak://notice", (event) => set("notice", event.payload));
}
