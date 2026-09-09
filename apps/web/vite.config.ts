import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // In development the dashboard runs on Vite and the API on wrangler. The
    // proxy keeps them same-origin, matching production where one Worker serves
    // both — so the device-approval path never needs CORS in either setting.
    proxy: {
      "/api": "http://localhost:8787",
      "/auth": "http://localhost:8787",
    },
  },
});
