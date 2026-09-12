/**
 * The recording overlay.
 *
 * A small pill that appears while dictating and shows that the microphone is
 * live. Bars follow the voice — quiet when you are quiet, moving when you
 * speak — rather than looping a fake animation.
 */

import { listen } from "@tauri-apps/api/event";

type Phase = "idle" | "listening" | "thinking" | "notice";

const BAR_COUNT = 5;

export function mountOverlay(root: HTMLElement): void {
  root.innerHTML = `
    <div class="pill" data-phase="idle">
      <div class="waveform" aria-hidden="true">${Array.from({ length: BAR_COUNT }, () => "<span></span>").join("")}</div>
      <span class="text" role="status" aria-live="polite"></span>
    </div>
  `;

  const pill = root.querySelector<HTMLElement>(".pill")!;
  const text = root.querySelector<HTMLElement>(".text")!;
  const bars = [...root.querySelectorAll<HTMLElement>(".waveform span")];
  const heights = new Float64Array(BAR_COUNT).fill(0.12);
  const phases = bars.map((_, index) => index * 1.37);
  let envelope = 0;

  const flatten = () => {
    envelope = 0;
    bars.forEach((bar, index) => {
      heights[index] = 0.12;
      bar.style.animation = "none";
      bar.style.transform = "scaleY(0.12)";
    });
  };

  const set = (phase: Phase, message: string) => {
    pill.dataset.phase = phase;
    text.textContent = message;
    if (phase !== "listening") flatten();
  };

  void listen("weldspeak://listening", () => set("listening", ""));

  void listen<number>("weldspeak://level", (event) => {
    if (pill.dataset.phase !== "listening") return;
    const raw = Math.max(0, event.payload);
    const db = 20 * Math.log10(Math.max(raw, 1e-5));
    // Conversational speech sits around -30 dB on a laptop mic; whisper lower.
    const voice = Math.max(0, Math.min(1, (db + 48) / 40));
    envelope = voice > envelope ? voice : envelope * 0.72 + voice * 0.28;

    bars.forEach((bar, index) => {
      const phase = (phases[index] ?? 0) + 0.28 + envelope * 2.1;
      phases[index] = phase;
      const wobble = 0.38 + 0.62 * Math.abs(Math.sin(phase));
      const target = 0.1 + envelope * wobble;
      const next = (heights[index] ?? 0.12) * 0.4 + target * 0.6;
      heights[index] = next;
      bar.style.animation = "none";
      bar.style.transform = `scaleY(${next.toFixed(3)})`;
    });
  });

  void listen("weldspeak://thinking", () => set("thinking", ""));
  void listen("weldspeak://done", () => set("idle", ""));
  void listen<string>("weldspeak://notice", (event) => set("notice", event.payload));
}
