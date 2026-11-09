//! `store.json`（version 2）：多便签持久化。
//!
//! 结构：
//! ```json
//! {
//!   "version": 2,
//!   "settings": { "autostart": true, "defaultStyle": { "color": "cream", "theme": "light", "fontSize": "md", "opacity": 0.97 } },
//!   "notes": [
//!     {
//!       "id": "uuid", "text": "正文", "todos": [],
//!       "color": "cream", "theme": "light", "fontSize": "md", "opacity": 0.97,
//!       "pinned": false, "collapsed": false, "visible": true,
//!       "bounds": { "x": 0, "y": 0, "width": 640, "height": 840 }
//!     }
//!   ]
//! }
//! ```
//!
//! 旧版（单便签 `{ text, todos, settings, window[, style] }`）在启动时无损迁移成 v2：
//! 迁移前先把原文件备份成 `store.v1.bak.json`（已存在则不覆盖），迁移后立刻按 v2 落盘，
//! 因此重复启动不会再走迁移分支——幂等。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 当前数据格式版本
pub const CURRENT_VERSION: u32 = 2;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TodoItem {
    pub id: String,
    pub text: String,
    pub done: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 开机自启（每次启动会和系统真实状态同步）
    pub autostart: bool,
    /// 新建便签时的默认外观
    pub default_style: NoteStyle,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            autostart: false,
            default_style: NoteStyle::default(),
        }
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

/// 标题长度上限（字符数），与前端输入框 maxLength 保持一致
pub const MAX_TITLE_CHARS: usize = 60;

/// 归一化标题：去掉首尾空白、截断到上限，全空回落到「便签」
pub fn normalize_title(raw: &str) -> String {
    let trimmed = raw.trim();
    let limited: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    if limited.is_empty() {
        "便签".to_string()
    } else {
        limited
    }
}

/// 单张便签：标题 / 内容 / 外观 / 置顶 / 可见性 / 几何全部独立
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub text: String,
    pub todos: Vec<TodoItem>,
    pub color: String,
    pub theme: String,
    pub font_size: String,
    pub opacity: f64,
    pub pinned: bool,
    /// 折叠成条：窗口只留标题栏高度（约 48 逻辑像素）。
    /// 注意 `bounds` 始终保存**展开态**几何，折叠只改窗口实际尺寸，不把 48px 写进 bounds。
    pub collapsed: bool,
    /// 启动时只恢复 visible = true 的便签；关闭 / 隐藏会置为 false
    pub visible: bool,
    pub bounds: Option<WindowGeometry>,
}

impl Default for Note {
    fn default() -> Self {
        Self {
            id: new_id(),
            title: "便签".to_string(),
            text: String::new(),
            todos: Vec::new(),
            color: "cream".to_string(),
            theme: "light".to_string(),
            font_size: "md".to_string(),
            opacity: 0.97,
            pinned: false,
            collapsed: false,
            visible: true,
            bounds: None,
        }
    }
}

impl Note {
    pub fn style(&self) -> NoteStyle {
        NoteStyle {
            color: self.color.clone(),
            theme: self.theme.clone(),
            font_size: self.font_size.clone(),
            opacity: self.opacity,
        }
    }

    pub fn apply_style(&mut self, style: &NoteStyle) {
        self.color = style.color.clone();
        self.theme = style.theme.clone();
        self.font_size = style.font_size.clone();
        self.opacity = style.opacity;
    }

    fn normalize(&mut self) {
        self.title = normalize_title(&self.title);
        let style = self.style().normalized();
        self.apply_style(&style);
        if !self.opacity.is_finite() {
            self.opacity = 0.97;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub version: u32,
    pub settings: Settings,
    pub notes: Vec<Note>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            settings: Settings::default(),
            notes: Vec::new(),
        }
    }
}

impl Store {
    /// 每次写盘前统一归一化：版本、样式白名单、id 非空且唯一
    pub fn normalize(&mut self) {
        self.version = CURRENT_VERSION;
        self.settings.default_style = self.settings.default_style.clone().normalized();

        let mut seen = std::collections::HashSet::new();
        for note in &mut self.notes {
            note.normalize();
            if note.id.trim().is_empty() || !seen.insert(note.id.clone()) {
                note.id = new_id();
                seen.insert(note.id.clone());
            }
        }
    }
}

// ---------------------------------------------------------------- 旧版数据

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacySettings {
    always_on_top: bool,
    autostart: bool,
}

/// 改造前的单便签格式；`style` 是任务二追加的字段，可能不存在。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyStore {
    text: String,
    todos: Vec<TodoItem>,
    settings: LegacySettings,
    window: Option<WindowGeometry>,
    style: Option<NoteStyle>,
}

pub struct LoadResult {
    pub store: Store,
    /// 本次启动是否执行了 v1 → v2 迁移
    pub migrated: bool,
}

/// 读取 store.json：v2 直接解析；旧格式走迁移（先备份），失败则回落默认值。
pub fn load(path: &Path) -> LoadResult {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("[store] 读取 {} 失败：{err}", path.display());
            }
            return LoadResult {
                store: Store::default(),
                migrated: false,
            };
        }
    };

    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("[store] 解析 {} 失败，使用默认数据：{err}", path.display());
            return LoadResult {
                store: Store::default(),
                migrated: false,
            };
        }
    };

    if is_v2(&value) {
        match serde_json::from_value::<Store>(value) {
            Ok(mut store) => {
                store.normalize();
                LoadResult {
                    store,
                    migrated: false,
                }
            }
            Err(err) => {
                eprintln!("[store] v2 数据解析失败，使用默认数据：{err}");
                LoadResult {
                    store: Store::default(),
                    migrated: false,
                }
            }
        }
    } else {
        let legacy: LegacyStore = serde_json::from_value(value).unwrap_or_default();
        if let Err(err) = backup_v1(path) {
            eprintln!("[store] 备份 v1 数据失败：{err}");
        }
        LoadResult {
            store: migrate(legacy),
            migrated: true,
        }
    }
}

/// v2 的判定只看 `notes` 是不是数组（v1 没有该字段），不依赖 version 是否合法
fn is_v2(value: &Value) -> bool {
    value.get("notes").map(|v| v.is_array()).unwrap_or(false)
}

fn migrate(legacy: LegacyStore) -> Store {
    let style = legacy.style.unwrap_or_default().normalized();
    let note = Note {
        id: new_id(),
        title: "便签".to_string(),
        text: legacy.text,
        todos: legacy.todos,
        color: style.color.clone(),
        theme: style.theme.clone(),
        font_size: style.font_size.clone(),
        opacity: style.opacity,
        pinned: legacy.settings.always_on_top,
        collapsed: false,
        visible: true,
        bounds: legacy.window,
    };

    let mut store = Store {
        version: CURRENT_VERSION,
        settings: Settings {
            autostart: legacy.settings.autostart,
            default_style: style,
        },
        notes: vec![note],
    };
    store.normalize();
    store
}

/// 迁移前备份旧文件；已存在备份时不覆盖，保证原始 v1 数据可回溯
fn backup_v1(path: &Path) -> std::io::Result<()> {
    let backup: PathBuf = path.with_file_name("store.v1.bak.json");
    if backup.exists() {
        return Ok(());
    }
    std::fs::copy(path, &backup).map(|_| ())
}

// ---------------------------------------------------------------- 落盘

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

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
