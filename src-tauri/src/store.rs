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

/// 便签外观。默认值必须和改造前的奶油色观感一致。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NoteStyle {
    pub color: String,
    pub theme: String,
    pub font_size: String,
    /// 卡片表面透明度（0.35–1），只影响底色和边框，不影响文字
    pub opacity: f64,
}

impl Default for NoteStyle {
    fn default() -> Self {
        Self {
            color: "cream".to_string(),
            theme: "light".to_string(),
            font_size: "md".to_string(),
            opacity: 0.97,
        }
    }
}

impl NoteStyle {
    /// 归一化非法值：配色 / 主题 / 字号走白名单，透明度 clamp 到 35%–100%
    pub fn normalized(mut self) -> Self {
        const COLORS: [&str; 6] = ["cream", "mint", "rose", "sky", "sand", "graphite"];
        const FONT_SIZES: [&str; 3] = ["sm", "md", "lg"];

        if !COLORS.contains(&self.color.as_str()) {
            self.color = "cream".to_string();
        }
        if self.theme != "light" && self.theme != "dark" {
            self.theme = "light".to_string();
        }
        if !FONT_SIZES.contains(&self.font_size.as_str()) {
            self.font_size = "md".to_string();
        }
        if !self.opacity.is_finite() {
            self.opacity = 0.97;
        }
        self.opacity = self.opacity.clamp(0.35, 1.0);
        self
    }
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
    /// 便签外观（配色 / 字号 / 主题 / 透明度）
    pub style: NoteStyle,
}

/// 读取失败（首次运行 / 文件损坏）一律回落到默认值，不阻塞启动
pub fn load(path: &Path) -> Store {
    match std::fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<Store>(&raw) {
            Ok(mut data) => {
                data.style = data.style.clone().normalized();
                data
            }
            Err(err) => {
                eprintln!("[store] 解析 {} 失败，使用默认数据：{err}", path.display());
                Store::default()
            }
        },
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
