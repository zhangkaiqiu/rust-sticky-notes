import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 2 期望固定端口；dev 时由 src-tauri/tauri.conf.json 的 devUrl 指向这里。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: false,
    watch: {
      // Rust 侧由 tauri dev 自己监听，避免 Vite 重复触发；
      // *.tmpdir / *.tmp 是编辑器原子写留下的临时文件，被占用时 chokidar 会 EBUSY 崩掉 dev server
      ignored: ["**/src-tauri/**", "**/*.tmpdir", "**/*.tmpdir/**", "**/*.tmp"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // WebView2 基于 Chromium，可直接用较新的语法目标
    target: "chrome110",
    minify: "esbuild",
    sourcemap: false,
  },
});
