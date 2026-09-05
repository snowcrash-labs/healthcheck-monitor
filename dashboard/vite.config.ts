import { defineConfig } from "vite";
import solid from "@solidjs/vite-plugin";

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: { "/api": { target: "http://127.0.0.1:9840", changeOrigin: true } },
  },
  build: { target: "es2023", sourcemap: false, chunkSizeWarningLimit: 250 },
});
