import { defineConfig } from "vite";

// Tauri 期望固定端口；不清屏以便看到 Rust 报错
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/target/**"] } },
  build: { target: "es2021", outDir: "dist", emptyOutDir: true },
});
