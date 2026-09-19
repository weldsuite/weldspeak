import { mountHub } from "./hub.js";
import "./styles.css";

const root = document.getElementById("app")!;
void mountHub(root);
