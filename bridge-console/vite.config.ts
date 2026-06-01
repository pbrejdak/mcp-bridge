import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port; matches `devUrl` in src-tauri/tauri.conf.json.
const TAURI_DEV_PORT = 1420;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: TAURI_DEV_PORT,
    strictPort: true,
    host: "localhost",
    watch: {
      // Tauri's Rust shell lives in src-tauri/; don't reload the
      // frontend when those files change (tauri-cli watches them).
      ignored: ["**/src-tauri/**"],
    },
  },
}));
