import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte()],
  server: {
    host: "127.0.0.1",
    port: 5176,
    proxy: {
      "/console": {
        target: "http://127.0.0.1:8718",
        changeOrigin: false,
      },
      "/memory": {
        target: "http://127.0.0.1:8718",
        changeOrigin: false,
      },
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 4176,
  },
});
