//! rust-sticky-notes：无边框 + 透明背景 + 隐藏任务栏按钮的多便签应用。
//!
//! 设计要点：
//! - 数据全部放在一个 `store.json`（Tauri app data 目录），Rust 侧单一 `Mutex<Store>` 统一读写；
//! - 每张便签一个窗口，label = `note-<id>`，前端通过 `get_note_state` 只拿到自己那张；
//! - 前端只负责编辑，落盘统一走 Rust：文本 450ms / 外观 300ms / 窗口 400ms 防抖；
//! - 关窗只隐藏，进程常驻托盘；真正退出走托盘菜单或设置面板。

mod cli;
mod diagnostics;
mod store;
#[cfg(windows)]
mod win_desktop_pin;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, LogicalSize, Manager, Monitor, PhysicalPosition, PhysicalSize, RunEvent, State,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

use store::{normalize_title, Note, NoteStyle, Store, TodoItem, WindowGeometry};

/// 便签窗口 label 前缀；完整 label 为 `note-<uuid>`
const NOTE_LABEL_PREFIX: &str = "note-";

/// 折叠态窗口高度（逻辑像素）：只够放一行标题栏
const COLLAPSED_HEIGHT: f64 = 48.0;
/// 折叠态允许的最小高度（逻辑像素）：比 48 略低，留一点系统边框误差
const MIN_COLLAPSED_HEIGHT: f64 = 40.0;
/// 展开态的尺寸下限（逻辑像素）；折叠时必须先把这个下限降下来，否则窗口会被顶回 260
const MIN_EXPANDED_WIDTH: f64 = 240.0;
const MIN_EXPANDED_HEIGHT: f64 = 260.0;
/// 没有历史展开高度时的兜底高度（逻辑像素）
const DEFAULT_EXPANDED_HEIGHT: f64 = 420.0;

pub struct AppState {
    store: Mutex<Store>,
    path: std::path::PathBuf,
    /// 便携模式（exe 同目录有 portable.txt）；`--dump` 会把它写进 JSON
    portable: bool,
    /// 每个窗口几何变化的代次，用来做「安静后才能落盘」的 debounce
    geom_gen: Mutex<HashMap<String, u64>>,
    /// 当前托盘菜单的句柄。菜单对象必须一直存活（Windows 托盘仍引用它），
    /// 不能构建完当临时值用完就丢；刷新时整体替换。
    tray_menu: Mutex<Option<Menu<tauri::Wry>>>,
}

impl AppState {
    /// 所有写操作都走这里：持锁改内存 → 归一化 → 原子落盘。
    /// 锁覆盖到写文件结束，避免两个窗口的快照交叉覆盖。
    fn update<T>(&self, f: impl FnOnce(&mut Store) -> T) -> Result<T, String> {
        let mut guard = self.store.lock().map_err(|e| e.to_string())?;
        let out = f(&mut guard);
        guard.normalize();
        store::save(&self.path, &guard)?;
        Ok(out)
    }

    fn read<T>(&self, f: impl FnOnce(&Store) -> T) -> Result<T, String> {
        let guard = self.store.lock().map_err(|e| e.to_string())?;
        Ok(f(&guard))
    }

    /// 只落盘不改数据（退出前 / 迁移后）
    fn flush(&self) -> Result<(), String> {
        let mut guard = self.store.lock().map_err(|e| e.to_string())?;
        guard.normalize();
        store::save(&self.path, &guard)
    }
}

// ---------------------------------------------------------------- 小工具

fn note_label(id: &str) -> String {
    format!("{NOTE_LABEL_PREFIX}{id}")
}

fn note_id_from_label(label: &str) -> Option<String> {
    label
        .strip_prefix(NOTE_LABEL_PREFIX)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

fn require_note_id(window: &Window) -> Result<String, String> {
    note_id_from_label(window.label())
        .ok_or_else(|| format!("当前窗口不是便签窗口：{}", window.label()))
}

fn work_area_of(monitor: &Monitor) -> (i32, i32, u32, u32) {
    let area = monitor.work_area();
    (
        area.position.x,
        area.position.y,
        area.size.width,
        area.size.height,
    )
}

/// 中心点落在哪块显示器上，就返回哪块的工作区
fn work_area_at(monitors: &[Monitor], x: i32, y: i32) -> Option<(i32, i32, u32, u32)> {
    monitors
        .iter()
        .map(work_area_of)
        .find(|(ax, ay, aw, ah)| {
            x >= *ax && x <= ax + *aw as i32 && y >= *ay && y <= ay + *ah as i32
        })
}

fn center_in_area(area: (i32, i32, u32, u32), width: u32, height: u32) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    (
        ax + (aw.saturating_sub(width) / 2) as i32,
        ay + (ah.saturating_sub(height) / 2) as i32,
    )
}

/// 把窗口夹到工作区内，保证至少整块可见（任务栏不会被盖住）
fn clamp_to_area(
    area: (i32, i32, u32, u32),
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let max_x = (ax + aw as i32 - width as i32).max(ax);
    let max_y = (ay + ah as i32 - height as i32).max(ay);
    (x.clamp(ax, max_x), y.clamp(ay, max_y))
}

// ---------------------------------------------------------------- commands

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteState {
    note: Note,
    autostart: bool,
    store_path: String,
}

#[tauri::command]
fn get_note_state(window: Window, state: State<'_, AppState>) -> Result<NoteState, String> {
    let id = require_note_id(&window)?;
    let (note, autostart) = state.read(|s| {
        (
            s.notes.iter().find(|n| n.id == id).cloned(),
            s.settings.autostart,
        )
    })?;
    let note = note.ok_or_else(|| format!("便签 {id} 不存在"))?;
    Ok(NoteState {
        note,
        autostart,
        store_path: state.path.display().to_string(),
    })
}

#[tauri::command]
fn save_note_content(
    window: Window,
    state: State<'_, AppState>,
    text: String,
    todos: Vec<TodoItem>,
) -> Result<(), String> {
    let id = require_note_id(&window)?;
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.text = text;
            note.todos = todos;
        }
    })
}

#[tauri::command]
fn set_note_style(
    window: Window,
    state: State<'_, AppState>,
    style: NoteStyle,
) -> Result<NoteStyle, String> {
    let normalized = style.normalized();
    let id = require_note_id(&window)?;
    let applied = normalized.clone();
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.apply_style(&applied);
        }
    })?;
    Ok(normalized)
}

#[tauri::command]
fn set_note_title(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    title: String,
) -> Result<String, String> {
    let normalized = normalize_title(&title);
    let id = require_note_id(&window)?;
    let applied = normalized.clone();
    // 同步操作系统层面的窗口标题（Alt+Tab / 任务管理器），失败不影响主流程
    let _ = window.set_title(&applied);
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.title = applied;
        }
    })?;
    // 托盘菜单里的便签名要跟着改
    schedule_tray_refresh(&app);
    Ok(normalized)
}

/// 查询当前便签窗口**实际**的置顶状态（读 `WS_EX_TOPMOST`，不是内存里的 JSON）。
///
/// 前端用它校准图钉图标：只要图标与真实层级不一致，就以这个返回值为准。
/// 不做轮询，只在拿到快照 / 切换图钉后各读一次。
#[tauri::command]
fn get_note_always_on_top(app: AppHandle, window: Window) -> Result<bool, String> {
    let label = window.label().to_string();
    let webview = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("便签窗口不存在：{label}"))?;
    #[cfg(windows)]
    {
        Ok(win_desktop_pin::is_always_on_top(&webview))
    }
    #[cfg(not(windows))]
    {
        Ok(webview.is_always_on_top().unwrap_or(false))
    }
}

#[tauri::command]
fn set_note_pinned(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    pinned: bool,
) -> Result<bool, String> {
    let id = require_note_id(&window)?;
    let label = window.label().to_string();
    let webview = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("便签窗口不存在：{label}"))?;
    apply_pinned(&webview, &id, &state, pinned)
}

/// 切换置顶的**唯一**后端路径：UI 图钉、设置面板、`--set-pin` 全都走这里。
///
/// 幂等：值没变、且实际层级已经对得上，就什么都不做。
/// 这是运行时唯一的置顶入口，重复调 tao 的 `set_always_on_top(false)` 会走
/// `SetWindowPos(HWND_NOTOPMOST)`，副作用是把窗口顶到普通层最前面（见 win_desktop_pin
/// 顶部注释），表现就是后台便签突然盖住前台应用。所以必须先比对再动手。
/// 注意要同时比对"内存状态"和"真实 exstyle"：重启后可能出现两者不一致的情况，
/// 此时不能因为值没变就直接返回，否则永远修不回来。
fn apply_pinned(
    webview: &WebviewWindow,
    id: &str,
    state: &AppState,
    pinned: bool,
) -> Result<bool, String> {
    let current = state.read(|s| s.notes.iter().find(|n| n.id == id).map(|n| n.pinned))?;
    #[cfg(windows)]
    let actual_now = win_desktop_pin::is_always_on_top(webview);
    #[cfg(not(windows))]
    let actual_now = webview.is_always_on_top().unwrap_or(false);
    if current == Some(pinned) && actual_now == pinned {
        return Ok(actual_now);
    }

    webview
        .set_always_on_top(pinned)
        .map_err(|e| format!("设置置顶失败：{e}"))?;
    // 取消置顶后必须把窗口重新垫回"桌面之上、其它窗口之下"：
    // HWND_NOTOPMOST 会把它留在普通层最前面，不补这一下就会飘在前台应用之上。
    // 同时把**其它**未置顶便签压回去：激活动作会把同 owner 组的兄弟窗口一起抬起来。
    #[cfg(windows)]
    let actual = win_desktop_pin::set_user_pinned(webview, pinned);
    #[cfg(not(windows))]
    let actual = webview.is_always_on_top().unwrap_or(pinned);

    // 落盘的是"真实生效的状态"，避免图标说钉住、实际没钉住
    let id = id.to_string();
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.pinned = actual;
        }
    })?;
    Ok(actual)
}

/// 按 id 取窗口并切换置顶（CLI 用；与 UI 完全同一条后端路径）
fn apply_pinned_for(app: &AppHandle, id: &str, pinned: bool) -> Result<bool, String> {
    let label = note_label(id);
    let webview = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("便签窗口不存在：{label}（可能是一张被隐藏的便签）"))?;
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "应用状态尚未就绪".to_string())?;
    apply_pinned(&webview, id, &state, pinned)
}

/// 折叠 / 展开当前便签。
///
/// 关键约束：`bounds` 永远保存**展开态**几何。
/// - 折叠：先把窗口此刻的展开几何抓进 bounds，再把窗口压到 48 逻辑像素高；
/// - 展开：按 bounds 里存的展开高度恢复（宽度沿用当前宽度）。
#[tauri::command]
fn set_note_collapsed(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    collapsed: bool,
) -> Result<bool, String> {
    let id = require_note_id(&window)?;
    let label = window.label().to_string();
    let webview = app
        .get_webview_window(&label)
        .ok_or_else(|| "便签窗口不存在".to_string())?;

    let (current, stored_bounds) = state.read(|s| {
        s.notes
            .iter()
            .find(|n| n.id == id)
            .map(|n| (n.collapsed, n.bounds.clone()))
            .unwrap_or((false, None))
    })?;
    if current == collapsed {
        return Ok(collapsed);
    }

    let mut bounds = stored_bounds.unwrap_or_default();
    if collapsed {
        // 以窗口此刻的真实几何作为「展开态」保存
        if let (Ok(position), Ok(size)) = (webview.outer_position(), webview.outer_size()) {
            if size.width >= 200 && size.height >= 100 {
                bounds = WindowGeometry {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                };
            }
        }
    }

    let persisted = bounds.clone();
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.collapsed = collapsed;
            if persisted.width > 0 && persisted.height > 0 {
                note.bounds = Some(persisted);
            }
        }
    })?;

    apply_collapse_size(&webview, collapsed, Some(bounds.height))
        .map_err(|e| format!("调整窗口尺寸失败：{e}"))?;

    schedule_tray_refresh(&app);
    Ok(collapsed)
}

#[tauri::command]
async fn create_note(app: AppHandle) -> Result<String, String> {
    // 必须是 async：同步命令跑在主线程，而建窗口要等事件循环把 WebView2 创建完，
    // 主线程一阻塞就会死锁（表现为窗口建出来但是 about:blank）。
    create_note_via(&app)
}

/// 新建一张便签（数据 + 窗口 + 托盘刷新）。UI 命令、托盘、`--new-note` 共用这一条路径。
fn create_note_via(app: &AppHandle) -> Result<String, String> {
    let note = new_note_internal(app)?;
    create_note_window(app, &note, true).map_err(|e| format!("创建便签窗口失败：{e}"))?;
    // 托盘菜单补上这张新便签
    schedule_tray_refresh(app);
    Ok(note.id)
}

#[tauri::command]
fn delete_note(app: AppHandle, window: Window, state: State<'_, AppState>) -> Result<(), String> {
    let id = require_note_id(&window)?;
    delete_note_via(&app, &state, &id)
}

/// 删除一张便签（数据 + 窗口 + 托盘刷新）。UI 命令与 `--delete-note` 共用这一条路径。
fn delete_note_via(app: &AppHandle, state: &AppState, id: &str) -> Result<(), String> {
    let id_owned = id.to_string();
    let remaining = state.update(move |s| {
        s.notes.retain(|n| n.id != id_owned);
        s.notes.len()
    })?;

    if let Some(webview) = app.get_webview_window(&note_label(id)) {
        let _ = webview.destroy();
    }

    // 被删除的便签要从托盘列表里消失
    schedule_tray_refresh(app);

    if remaining == 0 {
        // 最后一张便签被删除：刷盘后退出，避免留下一个没有任何便签的托盘进程
        let _ = state.flush();
        app.exit(0);
    }
    Ok(())
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("开启开机自启失败：{e}"))?;
    } else {
        manager
            .disable()
            .map_err(|e| format!("关闭开机自启失败：{e}"))?;
    }

    // 以系统真实状态为准回写
    let actual = manager.is_enabled().unwrap_or(enabled);
    state.update(|s| s.settings.autostart = actual)?;
    Ok(actual)
}

#[tauri::command]
fn hide_note(window: Window, state: State<'_, AppState>) -> Result<(), String> {
    let id = require_note_id(&window)?;
    let _ = window.hide();
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.visible = false;
        }
    })
}

#[tauri::command]
fn quit_app(app: AppHandle, state: State<'_, AppState>) {
    let _ = state.flush();
    app.exit(0);
}

// ---------------------------------------------------------------- 新建 / 窗口

/// 按 defaultStyle 建一张新便签，窗口按已有便签做层叠偏移，并夹回可见区域
fn new_note_internal(app: &AppHandle) -> Result<Note, String> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| "应用状态尚未就绪".to_string())?;
    let (default_style, notes) = state.read(|s| (s.settings.default_style.clone(), s.notes.clone()))?;

    let mut note = Note::default();
    note.apply_style(&default_style.normalized());
    note.bounds = Some(cascade_bounds(app, &notes));

    state.update(|s| s.notes.push(note.clone()))?;
    Ok(note)
}

fn cascade_bounds(app: &AppHandle, notes: &[Note]) -> WindowGeometry {
    let monitors = app.available_monitors().unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten();
    let scale = primary.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);

    let width = (320.0 * scale).round().max(240.0) as u32;
    let height = (420.0 * scale).round().max(260.0) as u32;
    let step = (32.0 * scale).round() as i32;

    let fallback_area = primary
        .as_ref()
        .map(work_area_of)
        .or_else(|| monitors.first().map(work_area_of))
        .unwrap_or((0, 0, 1920, 1080));

    let base = notes.iter().rev().find_map(|n| n.bounds.as_ref());
    let visible_count = notes.iter().filter(|n| n.visible).count() as i32;

    let (mut x, mut y) = match base {
        // 层叠偏移：每多一张可见便签就再错开 32（逻辑像素换算成物理像素）
        Some(bounds) => (
            bounds.x + step * visible_count.max(1),
            bounds.y + step * visible_count.max(1),
        ),
        None => center_in_area(fallback_area, width, height),
    };

    let center_area = work_area_at(&monitors, x + width as i32 / 2, y + height as i32 / 2)
        .unwrap_or(fallback_area);
    let (clamped_x, clamped_y) = clamp_to_area(center_area, x, y, width, height);
    x = clamped_x;
    y = clamped_y;

    WindowGeometry {
        x,
        y,
        width,
        height,
    }
}

fn create_note_window(
    app: &AppHandle,
    note: &Note,
    focus: bool,
) -> tauri::Result<WebviewWindow> {
    let label = note_label(&note.id);
    if let Some(existing) = app.get_webview_window(&label) {
        return Ok(existing);
    }

    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(note.title.clone())
        .inner_size(320.0, 420.0)
        // 最小高度先按折叠态给（40），展开态的 260 在 restore_bounds 之后按状态再设，
        // 否则 260 的下限会把折叠窗口顶回去。
        .min_inner_size(MIN_EXPANDED_WIDTH, MIN_COLLAPSED_HEIGHT)
        .decorations(false)
        .transparent(true)
        .skip_taskbar(true)
        .always_on_top(note.pinned)
        .maximizable(false)
        .shadow(false)
        .visible(false)
        .focused(false)
        .build()?;

    restore_bounds(&window, note.bounds.as_ref());
    // 折叠态启动：永远存展开几何，只把窗口实际高度压到 48
    if note.collapsed {
        if let Err(err) = apply_collapse_size(&window, true, note.bounds.as_ref().map(|b| b.height)) {
            eprintln!("[window] 应用折叠尺寸失败：{err}");
        }
    } else {
        let _ = window.set_min_size(Some(LogicalSize::new(
            MIN_EXPANDED_WIDTH,
            MIN_EXPANDED_HEIGHT,
        )));
    }
    let _ = window.show();
    // 窗口是 visible:false 创建后再 show 的，Windows 上有一定概率把任务栏按钮带回来，
    // 这里显式再删一次（底层是 ITaskbarList::DeleteTab）
    let _ = window.set_skip_taskbar(true);

    attach_window_events(app, &window);

    // 抵抗 Win+D：把桌面窗口设成便签的 owner，再把便签垫到桌面之上、其它窗口之下。
    // 顺序很重要：必须在 show 之后（窗口已可见才有 z 序可言），并且在 focus 之前
    // （激活会把同 owner 组的兄弟窗口一起抬起来，见 win_desktop_pin 顶部 BUG 1 说明）。
    #[cfg(windows)]
    win_desktop_pin::install(&window, note.pinned);

    if focus {
        let _ = window.set_focus();
        // 激活之后同 owner 组的未置顶便签会被连带抬起，补一刀压回去
        #[cfg(windows)]
        win_desktop_pin::park_siblings_after_activation(&window);
    }

    Ok(window)
}

/// 恢复几何：大小按物理像素写回，位置夹到所在显示器的工作区；显示器没了就回主屏居中
fn restore_bounds(window: &WebviewWindow, bounds: Option<&WindowGeometry>) {
    let Some(geo) = bounds else {
        let _ = window.center();
        return;
    };

    // 明显异常的数据直接丢弃（例如手动改坏了 JSON）
    if geo.width < 200 || geo.height < 200 {
        let _ = window.center();
        return;
    }

    let _ = window.set_size(PhysicalSize::new(geo.width, geo.height));

    let monitors = window.available_monitors().unwrap_or_default();
    let center_x = geo.x + geo.width as i32 / 2;
    let center_y = geo.y + geo.height as i32 / 2;
    let Some(area) = work_area_at(&monitors, center_x, center_y) else {
        // 上次所在的显示器已经拔掉了，回到主屏中间
        let _ = window.center();
        return;
    };

    let (x, y) = clamp_to_area(area, geo.x, geo.y, geo.width, geo.height);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// 按折叠状态设置窗口尺寸与最小尺寸限制。
///
/// 折叠时必须**先**把最小高度降到 40，再缩窗口——否则 260 的下限会把它顶回去；
/// 展开时反过来：先恢复尺寸再抬下限。折叠只改窗口，不动 `bounds`。
fn apply_collapse_size(
    window: &WebviewWindow,
    collapsed: bool,
    expanded_height: Option<u32>,
) -> tauri::Result<()> {
    let scale = window.scale_factor()?;
    let current = window.inner_size()?;
    let width_logical = (current.width as f64 / scale).max(MIN_EXPANDED_WIDTH);

    if collapsed {
        window.set_min_size(Some(LogicalSize::new(
            MIN_EXPANDED_WIDTH,
            MIN_COLLAPSED_HEIGHT,
        )))?;
        window.set_size(LogicalSize::new(width_logical, COLLAPSED_HEIGHT))?;
    } else {
        let height_logical = expanded_height
            .map(|h| h as f64 / scale)
            .unwrap_or(DEFAULT_EXPANDED_HEIGHT)
            .max(MIN_EXPANDED_HEIGHT);
        window.set_size(LogicalSize::new(width_logical, height_logical))?;
        window.set_min_size(Some(LogicalSize::new(
            MIN_EXPANDED_WIDTH,
            MIN_EXPANDED_HEIGHT,
        )))?;
    }
    Ok(())
}

fn attach_window_events(app: &AppHandle, window: &WebviewWindow) {
    let app = app.clone();
    let label = window.label().to_string();
    let win = window.clone();

    window.on_window_event(move |event| match event {
        // 不用事件里带的坐标/尺寸：在 Windows 高 DPI 下，创建时的初始尺寸是逻辑像素，
        // 事件里的是物理像素，混着用会把窗口存小。统一在防抖后从窗口本身读一次。
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            schedule_geometry_persist(&app, &label);
        }
        // 便签被激活（用户点它、或程序 focus）时，Windows 会把同一 owner 组
        // （owner = Progman）的其它便签一起抬到普通层最前面。这正是"点 A 的图钉，
        // B 窜到前台应用之上"的直接原因，所以在激活之后把其它未置顶的便签压回去。
        WindowEvent::Focused(true) => {
            #[cfg(windows)]
            if let Some(win) = app.get_webview_window(&label) {
                win_desktop_pin::park_siblings_after_activation(&win);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            // 没有系统标题栏，这里只兜底：关窗 = 收进托盘
            api.prevent_close();
            let _ = win.hide();
            set_note_visible(&app, &label, false);
        }
        _ => {}
    });
}

fn set_note_visible(app: &AppHandle, label: &str, visible: bool) {
    let Some(id) = note_id_from_label(label) else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let _ = state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.visible = visible;
        }
    });
}

/// 拖动/缩放会高频触发，等 400ms 没有新事件再写盘（每个窗口独立计代次）
fn schedule_geometry_persist(app: &AppHandle, label: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let generation = {
        let Ok(mut map) = state.geom_gen.lock() else {
            return;
        };
        let entry = map.entry(label.to_string()).or_insert(0);
        *entry += 1;
        *entry
    };
    drop(state);

    let app = app.clone();
    let label = label.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let current = state
            .geom_gen
            .lock()
            .map(|map| map.get(&label).copied().unwrap_or(0))
            .unwrap_or(0);
        if current != generation {
            return;
        }
        drop(state);
        capture_geometry(&app, &label);
    });
}

/// 从窗口读当前几何（物理像素）写进该便签的 bounds
/// 从窗口读当前几何（物理像素）写进该便签的 bounds。
///
/// 折叠态特殊处理：窗口高度是 48 逻辑像素，但 `bounds` 必须保留展开态高度，
/// 所以折叠时只更新位置和宽度，高度沿用已保存的展开高度。
fn capture_geometry(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    if window.is_minimized().unwrap_or(false) {
        return;
    }
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let Some(id) = note_id_from_label(label) else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (collapsed, saved_height) = state
        .read(|s| {
            s.notes
                .iter()
                .find(|n| n.id == id)
                .map(|n| {
                    (
                        n.collapsed,
                        n.bounds.as_ref().map(|b| b.height).unwrap_or(0),
                    )
                })
                .unwrap_or((false, 0))
        })
        .unwrap_or((false, 0));

    // 最小化 / 隐藏时可能拿到 0，丢弃这次结果；折叠态高度本来就只有 ~48，跳过高度下限检查
    if size.width < 100 || (!collapsed && size.height < 100) {
        return;
    }

    let height = if collapsed {
        saved_height.max(200)
    } else {
        size.height
    };
    let geometry = WindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height,
    };
    let _ = state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.bounds = Some(geometry);
        }
    });
}

// ---------------------------------------------------------------- 调试命令行
//
// `--dump` / `--check` / `--set-pin` / `--new-note` 都是**隐藏**参数，只给
// `scripts/check-zorder.ps1` 与手工排查用，UI 里没有入口。
//
// 两条执行路径共用下面的 `run_cli`：
// 1. 当前没有实例在跑 → 本次进程自己启动应用，等所有便签窗口建好之后在 `setup` 末尾执行；
// 2. 已经有实例在跑 → 新进程把 argv 交给单实例插件，由**已运行的实例**执行后自己退出。
//    所以"另开一个进程"对用户是透明的，但退出码拿不到（插件统一 exit(0)），
//    脚本一律以 %TEMP% 下的结果文件为准。

/// 本次进程启动时携带的诊断命令（没有则为 None）
static LAUNCH_CLI: std::sync::OnceLock<Option<cli::Cli>> = std::sync::OnceLock::new();

/// 执行一次诊断命令。
///
/// `forwarded = true` 表示 argv 由单实例插件转发而来，此刻正处在
/// `WM_COPYDATA` 的窗口过程里（主线程 + 消息处理中）：只有"建窗口"必须挪到
/// 异步任务里，其余采集/断言都是纯读，当场做即可。
fn run_cli(app: &AppHandle, command: &cli::Cli, forwarded: bool) {
    if command.new_note {
        if forwarded {
            let app = app.clone();
            let rest = command.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(err) = create_note_via(&app) {
                    eprintln!("[diag] --new-note 失败：{err}");
                }
                run_cli_after_note(&app, &rest);
            });
            return;
        }
        if let Err(err) = create_note_via(app) {
            eprintln!("[diag] --new-note 失败：{err}");
        }
    }
    run_cli_after_note(app, command);
}

/// `--new-note` 之后的部分：删除便签 → 切置顶 → dump / check
fn run_cli_after_note(app: &AppHandle, command: &cli::Cli) {
    if let Some(id) = command.delete_note.clone() {
        let result = app
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态尚未就绪".to_string())
            .and_then(|state| delete_note_via(app, &state, &id));
        let line = match result {
            Ok(()) => format!("已删除便签 {id}"),
            Err(err) => format!("--delete-note 失败：{err}"),
        };
        println!("[diag] {line}");
        diagnostics::console_notice(&line);
    }

    if let Some((target, pinned)) = command.set_pin.clone() {
        let result = resolve_pin_target(app, &target)
            .and_then(|id| apply_pinned_for(app, &id, pinned).map(|actual| (id, actual)));
        let line = match result {
            Ok((id, actual)) => format!("已设置置顶：note {id} pinned={pinned} topmost={actual}"),
            Err(err) => format!("--set-pin 失败：{err}"),
        };
        println!("[diag] {line}");
        diagnostics::console_notice(&line);
    }

    if !command.check && !command.dump {
        return;
    }

    let snapshot = diagnostics::collect(app);
    let dump_path = command
        .out
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(diagnostics::default_dump_path);
    let mut exit_code = 0;

    if command.check {
        match diagnostics::run_check(&snapshot, &dump_path) {
            Ok(outcome) => {
                let summary = if outcome.ok {
                    format!("PASS {}/{}", outcome.passed, outcome.total)
                } else {
                    let bad: Vec<String> = outcome
                        .checks
                        .iter()
                        .filter(|item| item.status == "fail")
                        .map(|item| format!("{}：{}", item.name, item.detail))
                        .collect();
                    format!("FAIL: {}", bad.join("；"))
                };
                println!("[diag] {summary}");
                diagnostics::console_notice(&summary);
                if !outcome.ok {
                    exit_code = 1;
                }
            }
            Err(err) => {
                eprintln!("[diag] 写入检查结果失败：{err}");
                exit_code = 2;
            }
        }
    } else {
        match diagnostics::write_dump(&snapshot, &dump_path) {
            Ok(()) => {
                let line = format!("已写入 {}", dump_path.display());
                println!("[diag] {line}");
                diagnostics::console_notice(&line);
            }
            Err(err) => eprintln!("[diag] --dump 失败：{err}"),
        }
    }

    if command.exit_after_dump {
        if let Some(state) = app.try_state::<AppState>() {
            let _ = state.flush();
        }
        app.exit(exit_code);
    }
}

/// `--set-pin auto` 的目标：z 序最靠前的那张便签；一张窗口都没有就退化成数据里第一张
fn resolve_pin_target(app: &AppHandle, target: &str) -> Result<String, String> {
    if target != "auto" {
        return Ok(target.to_string());
    }
    let snapshot = diagnostics::collect(app);
    snapshot
        .windows
        .first()
        .map(|window| window.note_id.clone())
        .or_else(|| snapshot.notes.first().map(|note| note.id.clone()))
        .ok_or_else(|| "当前没有可操作的便签".to_string())
}

// ---------------------------------------------------------------- 托盘

/// 单实例锁回调：第二次双击 exe 时唤回已有便签。
/// 只处理「已经存在」的窗口——这里不能建窗口（回调不在主线程，WebView2 必须主线程创建），
/// 被隐藏到托盘的便签用托盘「显示全部便签」来恢复即可。
/// 只在「确实需要」时 show / unminimize。
///
/// 对已经可见的窗口调 `show()` 会重新激活它、把它抬到最前——这是"后台便签自己跳到前台"
/// 的一大来源。所有唤回路径统一走这里，已可见的窗口不会被重新抬起。
/// 返回是否真的显示过（被隐藏 / 最小化的窗口）。
fn ensure_window_visible(window: &WebviewWindow) -> bool {
    let mut changed = false;
    if window.is_minimized().unwrap_or(false) {
        let _ = window.unminimize();
        changed = true;
    }
    if !window.is_visible().unwrap_or(true) {
        let _ = window.show();
        // 窗口是 visible:false 创建后再 show 的，Windows 上有一定概率把任务栏按钮带回来
        let _ = window.set_skip_taskbar(true);
        changed = true;
    }
    changed
}

fn focus_existing_notes(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let notes = state.read(|s| s.notes.clone()).unwrap_or_default();
    let mut first = None;
    let mut shown = Vec::new();

    for note in &notes {
        if let Some(window) = app.get_webview_window(&note_label(&note.id)) {
            ensure_window_visible(&window);
            if first.is_none() {
                first = Some(window);
            }
            shown.push(note.id.clone());
        }
    }

    if !shown.is_empty() {
        let _ = state.update(|s| {
            for note in &mut s.notes {
                if shown.contains(&note.id) {
                    note.visible = true;
                }
            }
        });
    }

    // 只把第一张便签唤到焦点，其余窗口保持原层级，避免整排便签一起跳到前台
    if let Some(window) = first {
        let _ = window.set_focus();
    }
}

fn show_all_notes(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let notes = state.read(|s| s.notes.clone()).unwrap_or_default();
    let mut last = None;

    for note in &notes {
        if let Some(window) = app.get_webview_window(&note_label(&note.id)) {
            // 已可见的窗口不重复 show，避免把整排便签抬到前台应用之上
            ensure_window_visible(&window);
            last = Some(window);
        } else {
            match create_note_window(app, note, false) {
                Ok(window) => last = Some(window),
                Err(err) => eprintln!("[window] 恢复便签 {} 失败：{err}", note.id),
            }
        }
    }

    let _ = state.update(|s| {
        for note in &mut s.notes {
            note.visible = true;
        }
    });

    if let Some(window) = last {
        let _ = window.set_focus();
    }
}

fn hide_all_notes(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let notes = state.read(|s| s.notes.clone()).unwrap_or_default();
    for note in &notes {
        if let Some(window) = app.get_webview_window(&note_label(&note.id)) {
            let _ = window.hide();
        }
    }
    let _ = state.update(|s| {
        for note in &mut s.notes {
            note.visible = false;
        }
    });
}

fn flush_and_exit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.flush();
    }
    app.exit(0);
}

/// 在资源管理器里选中这个文件（托盘「诊断：导出窗口状态」用）。
///
/// `/select,` 后面的整串算一个参数，带空格的路径不需要自己加引号。
fn reveal_in_explorer(path: &Path) {
    if let Err(err) = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
    {
        eprintln!("[diag] 打开资源管理器失败：{err}");
    }
}

/// 托盘菜单里最多列出的便签数，超出的用「…另有 N 张」占位
const MAX_TRAY_NOTES: usize = 20;

/// 托盘里的便签显示名：默认标题（空 / 「便签」）补上序号，避免一堆同名项
fn tray_note_title(note: &Note, index: usize) -> String {
    let title = note.title.trim();
    if title.is_empty() || title == "便签" {
        format!("便签 {}", index + 1)
    } else {
        title.to_string()
    }
}

/// 按当前便签列表构建托盘菜单：便签项（最多 20 条）→ 分隔线 → 原有动作。
/// 返回的 Menu 由调用方保存（托盘 + AppState 各持一份），不要当临时值丢掉。
fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let notes = app
        .try_state::<AppState>()
        .and_then(|state| state.read(|s| s.notes.clone()).ok())
        .unwrap_or_default();

    // 便签项要先全部创建完，下面才能安全地借用它们拼 items
    let mut note_items: Vec<MenuItem<tauri::Wry>> = Vec::new();
    for (index, note) in notes.iter().take(MAX_TRAY_NOTES).enumerate() {
        let label = tray_note_title(note, index);
        note_items.push(MenuItem::with_id(
            app,
            format!("note:{}", note.id),
            label,
            true,
            None::<&str>,
        )?);
    }

    let more_item = if notes.len() > MAX_TRAY_NOTES {
        Some(MenuItem::with_id(
            app,
            "more",
            format!("…另有 {} 张", notes.len() - MAX_TRAY_NOTES),
            false,
            None::<&str>,
        )?)
    } else {
        None
    };

    let separator = PredefinedMenuItem::separator(app)?;
    let new_item = MenuItem::with_id(app, "new", "新建便签", true, None::<&str>)?;
    let show_item = MenuItem::with_id(app, "show_all", "显示全部便签", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide_all", "隐藏全部便签", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    // 调试入口：导出窗口状态（极少数场景才用，所以放在最底下、单独一条分隔线之后）
    let diag_separator = PredefinedMenuItem::separator(app)?;
    let diag_item = MenuItem::with_id(
        app,
        "diag_dump",
        "诊断：导出窗口状态",
        true,
        None::<&str>,
    )?;

    let mut items: Vec<&dyn IsMenuItem<tauri::Wry>> = Vec::new();
    if !note_items.is_empty() {
        for item in &note_items {
            items.push(item);
        }
        if let Some(more) = &more_item {
            items.push(more);
        }
        items.push(&separator);
    }
    items.push(&new_item);
    items.push(&show_item);
    items.push(&hide_item);
    items.push(&quit_item);
    items.push(&diag_separator);
    items.push(&diag_item);

    Menu::with_items(app, &items)
}

/// 刷新托盘菜单（新建 / 改名 / 删除 / 折叠后都要调）。
///
/// 菜单构建内部会 `run_on_main_thread` 并阻塞等结果，如果当前已经在主线程
/// （同步 command / 菜单回调）会死锁，所以统一丢到异步运行时里执行。
fn schedule_tray_refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let menu = match build_tray_menu(&app) {
            Ok(menu) => menu,
            Err(err) => {
                eprintln!("[tray] 构建菜单失败：{err}");
                return;
            }
        };
        // 新菜单先存进 AppState，保证托盘切换期间对象一直存活
        if let Ok(mut slot) = state.tray_menu.lock() {
            *slot = Some(menu.clone());
        }
        if let Some(tray) = app.tray_by_id("main-tray") {
            if let Err(err) = tray.set_menu(Some(menu)) {
                eprintln!("[tray] 刷新菜单失败：{err}");
            }
        }
    });
}

/// 托盘点击某张便签：已有窗口直接 show + focus；窗口还不存在（启动时隐藏的那张）就按需重建
fn show_note_by_id(app: &AppHandle, id: &str) {
    if let Some(window) = app.get_webview_window(&note_label(id)) {
        // 只处理这一张：已可见的窗口不再 show（避免抬起其它便签），直接聚焦即可
        ensure_window_visible(&window);
        let _ = window.set_focus();
        if let Some(state) = app.try_state::<AppState>() {
            let id = id.to_string();
            let _ = state.update(|s| {
                if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
                    note.visible = true;
                }
            });
        }
        return;
    }

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let note = state
        .read(|s| s.notes.iter().find(|n| n.id == id).cloned())
        .ok()
        .flatten();
    let Some(note) = note else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = create_note_window(&app, &note, true) {
            eprintln!("[tray] 恢复便签窗口失败：{err}");
        }
    });
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();
    let menu = build_tray_menu(&handle)?;
    // 先保存一份菜单句柄，避免菜单对象在托盘接管前被释放
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut slot) = state.tray_menu.lock() {
            *slot = Some(menu.clone());
        }
    }

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("便签")
        .menu(&menu)
        // 左键单击唤回全部便签，右键才出菜单
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();

            // 便签列表项：id 形如 note:<uuid>
            if let Some(note_id) = id.strip_prefix("note:") {
                let note_id = note_id.to_string();
                let app = app.clone();
                tauri::async_runtime::spawn(async move { show_note_by_id(&app, &note_id) });
                return;
            }

            match id.as_str() {
                "new" => {
                    // 托盘菜单回调在主线程，而建窗口需要事件循环继续跑，必须挪到后台线程
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        match new_note_internal(&app) {
                            Ok(note) => {
                                if let Err(err) = create_note_window(&app, &note, true) {
                                    eprintln!("[tray] 新建便签窗口失败：{err}");
                                }
                                schedule_tray_refresh(&app);
                            }
                            Err(err) => eprintln!("[tray] 新建便签失败：{err}"),
                        }
                    });
                }
                "show_all" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { show_all_notes(&app) });
                }
                "hide_all" => hide_all_notes(app),
                "quit" => flush_and_exit(app),
                "diag_dump" => {
                    // 只读状态、几十毫秒就能跑完，直接在主线程做（托盘回调本来就在主线程）
                    match diagnostics::write_dump(
                        &diagnostics::collect(app),
                        &diagnostics::default_dump_path(),
                    ) {
                        Ok(()) => {
                            let path = diagnostics::default_dump_path();
                            println!("[diag] 已写入 {}", path.display());
                            reveal_in_explorer(&path);
                        }
                        Err(err) => eprintln!("[tray] 导出窗口状态失败：{err}"),
                    }
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle().clone();
                tauri::async_runtime::spawn(async move { show_all_notes(&app) });
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

// ---------------------------------------------------------------- 便携模式

/// 解析数据目录：
/// - exe 同目录存在 `portable.txt` → 便携模式，数据跟着 exe 走；
/// - 否则用系统默认的 AppData 目录（行为与旧版完全一致）。
///
/// 便携模式首次启动时，如果 exe 同目录还没有 `store.json`，会把 AppData 里那份复制过来，
/// 避免从安装版切过来丢数据；反向不做任何同步。
fn resolve_data_dir(default_dir: &Path) -> (PathBuf, bool) {
    let Ok(exe) = std::env::current_exe() else {
        return (default_dir.to_path_buf(), false);
    };
    let Some(exe_dir) = exe.parent() else {
        return (default_dir.to_path_buf(), false);
    };
    if !exe_dir.join("portable.txt").is_file() {
        return (default_dir.to_path_buf(), false);
    }

    let portable_store = exe_dir.join("store.json");
    if !portable_store.exists() {
        let source = default_dir.join("store.json");
        if source.exists() {
            match std::fs::copy(&source, &portable_store) {
                Ok(_) => println!(
                    "[store] 便携模式首次启动：已从 {} 复制数据",
                    source.display()
                ),
                Err(err) => eprintln!("[store] 便携模式复制数据失败：{err}"),
            }
        }
    }

    (exe_dir.to_path_buf(), true)
}

// ---------------------------------------------------------------- entry

pub fn run() {
    // 命令行只解析一次：没有已运行实例时，命令要等 setup 里所有窗口建完才执行
    let argv: Vec<String> = std::env::args().collect();
    let _ = LAUNCH_CLI.set(cli::parse(&argv));

    tauri::Builder::default()
        // 单实例锁必须是「第一个注册」的插件（插件自身的硬性要求）。
        // 第二次双击 exe 时：新进程由插件直接退出，回调在已存在的实例里把便签唤回前台。
        // 注意按应用 identifier 判定，所以便携模式的两份拷贝同机运行也只会有一份活着。
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            // 诊断参数（--dump / --check / --set-pin / --new-note）：
            // **只执行命令，不唤回便签**——唤回会 set_focus 把窗口抬起来，
            // 正好把待断言的层级搞乱。回调就在已运行实例的主线程上（WM_COPYDATA），
            // 所以可以直接同步执行。
            if let Some(mut command) = cli::parse(&argv) {
                // `--out` 的相对路径按"发起命令的那个进程"的 cwd 解析；
                // 已运行实例自己的 cwd 可能完全不同（比如从资源管理器启动）
                if let Some(out) = command.out.clone() {
                    if Path::new(&out).is_relative() {
                        command.out = Some(Path::new(&cwd).join(out).display().to_string());
                    }
                }
                let app = app.clone();
                run_cli(&app, &command, true);
                return;
            }

            let app = app.clone();
            // 回调不保证在主线程，窗口操作统一挪回主线程执行
            let _ = app.clone().run_on_main_thread(move || {
                focus_existing_notes(&app);
            });
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let default_dir = app.path().app_data_dir()?;
            let (data_dir, portable) = resolve_data_dir(&default_dir);
            std::fs::create_dir_all(&data_dir)?;
            let store_path = data_dir.join("store.json");

            if portable {
                println!("[store] 便携模式：exe 同目录存在 portable.txt");
            }
            // 排查数据到底写在哪儿：便携模式 / AppData 一眼可见
            println!("[store] 数据文件: {}", store_path.display());

            let loaded = store::load(&store_path);
            let mut data = loaded.store;
            if loaded.migrated {
                eprintln!("[store] 已从 v1 单便签格式迁移到 v2 多便签格式");
            }

            // 以系统真实的自启状态为准，避免 JSON 和注册表不一致
            data.settings.autostart = handle
                .autolaunch()
                .is_enabled()
                .unwrap_or(data.settings.autostart);

            // 首次运行（或用户把便签全删了）至少保证有一张
            if data.notes.is_empty() {
                let mut note = Note::default();
                note.apply_style(&data.settings.default_style.clone().normalized());
                note.bounds = Some(cascade_bounds(&handle, &[]));
                data.notes.push(note);
            }

            let visible_notes: Vec<Note> = data
                .notes
                .iter()
                .filter(|note| note.visible)
                .cloned()
                .collect();

            app.manage(AppState {
                store: Mutex::new(data),
                path: store_path,
                portable,
                geom_gen: Mutex::new(HashMap::new()),
                tray_menu: Mutex::new(None),
            });

            // 迁移后 / 首次运行立刻落盘：迁移幂等靠这一步把 v2 写回，下次不再迁移
            if let Some(state) = handle.try_state::<AppState>() {
                let _ = state.flush();
            }

            setup_tray(app)?;

            let mut last_window = None;
            for note in &visible_notes {
                match create_note_window(&handle, note, false) {
                    Ok(window) => last_window = Some(window),
                    Err(err) => eprintln!("[window] 创建便签 {} 失败：{err}", note.id),
                }
            }
            if let Some(window) = last_window {
                let _ = window.set_focus();
            }

            // 本次启动带了诊断参数（且没有已运行实例）：所有便签窗口都已建好，
            // 现在采集才不会是半成品。放在 setup 最后一行，不早于任何窗口创建。
            if let Some(command) = LAUNCH_CLI.get().and_then(|slot| slot.as_ref()) {
                run_cli(&handle, command, false);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_note_state,
            save_note_content,
            set_note_title,
            set_note_style,
            set_note_pinned,
            get_note_always_on_top,
            set_note_collapsed,
            create_note,
            delete_note,
            set_autostart,
            hide_note,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.flush();
                }
            }
        });
}
