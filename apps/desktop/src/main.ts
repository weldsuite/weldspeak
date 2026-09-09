import { mountOverlay } from "./overlay.js";
import { mountSettings } from "./settings.js";
import "./styles.css";

const root = document.getElementById("app")!;

// One bundle serves both windows; the overlay is addressed by hash so it needs
// no separate entry point or build output.
if (window.location.hash === "#/overlay") {
  document.body.classList.add("overlay-window");
  mountOverlay(root);
} else {
  void mountSettings(root);
}
