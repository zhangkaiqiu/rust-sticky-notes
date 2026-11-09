import { invoke } from "@tauri-apps/api/core";
import type { Store, TodoItem } from "./types";

/**
 * 所有前端 → 后端的调用集中在这里。
 * Tauri 2 默认把命令参数名按 camelCase 序列化（Rust 侧的 snake_case 参数
 * 在前端写作 camelCase，例如 `always_on_top` -> `alwaysOnTop`）。
 */
export const api = {
  loadState: () => invoke<Store>("load_state"),
  saveNote: (text: string, todos: TodoItem[]) =>
    invoke<void>("save_note", { text, todos }),
  setPinned: (pinned: boolean) => invoke<void>("set_pinned", { pinned }),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  getStorePath: () => invoke<string>("get_store_path"),
  hideWindow: () => invoke<void>("hide_window"),
  quitApp: () => invoke<void>("quit_app"),
};
