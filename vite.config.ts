import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    // 5473 instead of Vite's default 5173 — ../glovebox dev server owns 5173.
    // Must match devUrl in src-tauri/tauri.conf.json.
    port: 5473,
    strictPort: true,
  },
});
