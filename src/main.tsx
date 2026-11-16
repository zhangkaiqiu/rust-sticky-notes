import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import SettingsWindow from "./SettingsWindow";
import "./styles.css";

/**
 * 同一个 `index.html` 服务两种窗口：
 * - `index.html`                      → 便签窗口
 * - `index.html?settings=<noteId>`    → 该便签的设置面板窗口
 *
 * 用 URL 参数而不是窗口 label 来判断，因为前端拿 label 需要额外的 IPC。
 */
const root = document.getElementById("root");
if (!root) throw new Error("#root not found");

const params = new URLSearchParams(window.location.search);
const settingsNoteId = params.get("settings");

ReactDOM.createRoot(root).render(
  <StrictMode>
    {settingsNoteId ? <SettingsWindow noteId={settingsNoteId} /> : <App />}
  </StrictMode>
);
