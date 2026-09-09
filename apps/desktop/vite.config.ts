import { defineConfig } from "vite";

// Tauri serves this on a fixed port and watches for changes; the src-tauri
// directory is excluded so Rust edits do not trigger a frontend reload.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "es2022" },
});
