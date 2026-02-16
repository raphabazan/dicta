import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        widget: resolve(__dirname, "widget.html"),
        "prompt-input": resolve(__dirname, "prompt-input.html"),
        "tts-widget": resolve(__dirname, "tts-widget.html"),
        "tts-toast": resolve(__dirname, "tts-toast.html"),
        warning: resolve(__dirname, "warning.html"),
      },
    },
  },
  server: {
    host: host || false,
    port: 1420,
    strictPort: true,
    hmr: host
      ? {
          protocol: "ws",
          host: host,
          port: 1430,
        }
      : undefined,
  },
}));
