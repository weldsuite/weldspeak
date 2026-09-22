import "@fontsource-variable/inter/wght.css";
import "@fontsource/playfair-display/500.css";
import { mountHub } from "./hub.js";
import "./styles.css";

const root = document.getElementById("app")!;
void mountHub(root);
