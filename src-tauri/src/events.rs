//! 跨窗口事件通道。
//!
//! 这个应用是**多窗口**的：每条便签一个 `note-<id>` 窗口，每条便签还可能有一个
//! `settings-<id>` 面板窗口。两个窗口各自持有 React 状态，如果彼此不通知，
//! 就会出现这类问题（都真实发生过）：
//!
//! - 面板改了配色，便签窗口不刷新 → 用户以为没生效；
//! - 面板窗口被关掉，便签窗口的齿轮按钮还以为它开着 → 点一次没反应；
//! - 便签移动 / 折叠，面板没跟着走 → 面板飘在原地。
//!
//! 所以规矩定成一条：**状态以后端为准，变了就广播**。窗口收到自己的事件就重新读一次。
//! 前端不再靠本地 state 猜另一端的状态。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 便签数据变了（外观 / 标题 / 置顶 / 内容）。由便签窗口和面板窗口分别监听。
pub const NOTE_UPDATED: &str = "note-updated";

/// 设置面板的开关状态变了，便签窗口据此同步齿轮按钮的高亮。
pub const SETTINGS_STATE: &str = "settings-state";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteUpdatedPayload {
    note_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsStatePayload {
    note_id: String,
    open: bool,
}

/// 通知「某张便签的数据变了」。所有便签窗口和面板窗口都会收到，各自判断是不是自己。
pub fn note_updated(app: &AppHandle, note_id: &str) {
    let _ = app.emit(
        NOTE_UPDATED,
        NoteUpdatedPayload {
            note_id: note_id.to_string(),
        },
    );
}

/// 通知「某张便签的面板开了 / 关了」。
pub fn settings_state(app: &AppHandle, note_id: &str, open: bool) {
    let _ = app.emit(
        SETTINGS_STATE,
        SettingsStatePayload {
            note_id: note_id.to_string(),
            open,
        },
    );
}
