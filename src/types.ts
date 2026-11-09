import type { NoteStyle } from "./theme";

export type { NoteStyle } from "./theme";

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

export type Settings = {
  alwaysOnTop: boolean;
  autostart: boolean;
};

/** 与 Rust 侧 `store::Store` 一一对应（serde camelCase） */
export type Store = {
  text: string;
  todos: TodoItem[];
  settings: Settings;
  window: WindowGeometry | null;
  style: NoteStyle;
};
