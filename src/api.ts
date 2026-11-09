import { invoke } from "@tauri-apps/api/core";
import type { NoteState, NoteStyle, TodoItem } from "./types";

/**
 * 所有前端 → 后端的调用集中在这里。
 * Tauri 2 默认把命令参数名按 camelCase 序列化（Rust 侧的 snake_case 参数
 * 在前端写作 camelCase，例如 `always_on_top` -> `alwaysOnTop`）。
 *
 * 后端通过调用方窗口的 label（`note-<id>`）判断操作哪张便签，
 * 前端不需要、也不允许直接碰 store.json。
 */
export const api = {
  noteState: () => invoke<NoteState>("get_note_state"),
  saveContent: (text: string, todos: TodoItem[]) =>
    invoke<void>("save_note_content", { text, todos }),
  saveTitle: (title: string) => invoke<string>("set_note_title", { title }),
  saveStyle: (style: NoteStyle) => invoke<NoteStyle>("set_note_style", { style }),
  setPinned: (pinned: boolean) => invoke<void>("set_note_pinned", { pinned }),
  /** 折叠 / 展开当前便签：只改窗口实际尺寸，bounds 里始终是展开态几何 */
  setCollapsed: (collapsed: boolean) =>
    invoke<boolean>("set_note_collapsed", { collapsed }),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  createNote: () => invoke<string>("create_note"),
  deleteNote: () => invoke<void>("delete_note"),
  hideNote: () => invoke<void>("hide_note"),
  quitApp: () => invoke<void>("quit_app"),
};
