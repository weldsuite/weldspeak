import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { mountOverlay } from "./overlay.js";
import { mountSettings } from "./settings.js";
import "./styles.css";

const root = document.getElementById("app")!;

// The overlay is a second window of the same bundle. Identify it by label,
// not by URL hash: production WebView2 often drops the hash.
if (getCurrentWindow().label === "overlay") {
  document.body.classList.add("overlay-window");
  mountOverlay(root);
} else {
  const refresh = () => void mountSettings(root);
  void refresh();
  void listen("weldspeak://signed-in", refresh);
}
