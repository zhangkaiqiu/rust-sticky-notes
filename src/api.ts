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
  /** 保存正文 / 待办的分割比例；0 = 恢复按内容自适应。返回后端归一化后的实际值 */
  saveSplitRatio: (ratio: number) =>
    invoke<number>("set_note_split_ratio", { ratio }),
  saveTitle: (title: string) => invoke<string>("set_note_title", { title }),
  saveStyle: (style: NoteStyle) => invoke<NoteStyle>("set_note_style", { style }),
  setPinned: (pinned: boolean) =>
    invoke<boolean>("set_note_pinned", { pinned }),
  /** 回读窗口**实际**的置顶状态（后端读 WS_EX_TOPMOST），用于校准图钉图标 */
  alwaysOnTop: () => invoke<boolean>("get_note_always_on_top"),
  /** 折叠 / 展开当前便签：只改窗口实际尺寸，bounds 里始终是展开态几何 */
  setCollapsed: (collapsed: boolean) =>
    invoke<boolean>("set_note_collapsed", { collapsed }),
  /** 切换设置面板（后端按真实窗口是否存在决定开/关，前端不猜状态） */
  toggleSettings: () => invoke<boolean>("toggle_settings_panel"),
  /** 关闭设置面板 */
  closeSettings: () => invoke<void>("close_settings_panel"),
  /** 按内容高度调整面板窗口（高度跟着内容走，避免裁切或留白） */
  resizeSettings: (contentHeight: number) =>
    invoke<void>("resize_settings_panel", { contentHeight }),
  /** 当前窗口所属便签的 id（便签窗口 / 面板窗口都能问） */
  currentNoteId: () => invoke<string>("current_note_id"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  createNote: () => invoke<string>("create_note"),
  deleteNote: () => invoke<void>("delete_note"),
  hideNote: () => invoke<void>("hide_note"),
  quitApp: () => invoke<void>("quit_app"),
};
