import type { FontSize, NoteColor, NoteTheme } from "./theme";

export type { FontSize, NoteColor, NoteStyle, NoteTheme } from "./theme";

export type TodoItem = {
  id: string;
  text: string;
  done: boolean;
};

export type WindowGeometry = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/** 与 Rust 侧 `store::Note` 一一对应（serde camelCase） */
export type Note = {
  id: string;
  title: string;
  text: string;
  todos: TodoItem[];
  color: NoteColor;
  theme: NoteTheme;
  fontSize: FontSize;
  opacity: number;
  pinned: boolean;
  /** 折叠成条：只显示标题栏；bounds 始终是展开态几何 */
  collapsed: boolean;
  /**
   * 正文区 / 待办区的分割比例（待办区占比，0–1）。
   * `0` 是哨兵：表示用户没调过，走"按内容自适应"的默认布局。
   */
  splitRatio: number;
  visible: boolean;
  bounds: WindowGeometry | null;
};

/** `get_note_state` 的返回：当前窗口对应的那张便签 + 全局设置 */
export type NoteState = {
  note: Note;
  autostart: boolean;
  storePath: string;
};
