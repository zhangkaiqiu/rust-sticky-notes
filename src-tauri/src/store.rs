use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 图钉状态：窗口是否 alwaysOnTop
    pub always_on_top: bool,
    /// 开机自启（每次启动会和系统真实状态同步）
    pub autostart: bool,
}

/// 窗口位置/大小，单位是物理像素（Tauri 事件给的就是物理像素）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 整个应用的持久化数据，单文件 `store.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub text: String,
    pub todos: Vec<TodoItem>,
    pub settings: Settings,
    pub window: Option<WindowGeometry>,
}

/// 读取失败（首次运行 / 文件损坏）一律回落到默认值，不阻塞启动
pub fn load(path: &Path) -> Store {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|err| {
            eprintln!("[store] 解析 {} 失败，使用默认数据：{err}", path.display());
            Store::default()
        }),
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[store] 读取 {} 失败：{err}", path.display());
            }
            Store::default()
        }
    }
}

/// 先写临时文件再 rename，避免写一半崩溃把数据写坏
pub fn save(path: &Path, data: &Store) -> Result<(), String> {
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    }

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("替换数据文件失败：{e}"))?;
    Ok(())
}
