//! rust-sticky-notes：无边框 + 透明背景 + 隐藏任务栏按钮的多便签应用。
//!
//! 设计要点：
//! - 数据全部放在一个 `store.json`（Tauri app data 目录），Rust 侧单一 `Mutex<Store>` 统一读写；
//! - 每张便签一个窗口，label = `note-<id>`，前端通过 `get_note_state` 只拿到自己那张；
//! - 前端只负责编辑，落盘统一走 Rust：文本 450ms / 外观 300ms / 窗口 400ms 防抖；
//! - 关窗只隐藏，进程常驻托盘；真正退出走托盘菜单或设置面板。

mod store;
#[cfg(windows)]
mod win_desktop_pin;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, RunEvent, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

use store::{normalize_title, Note, NoteStyle, Store, TodoItem, WindowGeometry};

/// 便签窗口 label 前缀；完整 label 为 `note-<uuid>`
const NOTE_LABEL_PREFIX: &str = "note-";

pub struct AppState {
    store: Mutex<Store>,
    path: std::path::PathBuf,
    /// 每个窗口几何变化的代次，用来做「安静后才能落盘」的 debounce
    geom_gen: Mutex<HashMap<String, u64>>,
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
    Ok(normalized)
}

#[tauri::command]
fn set_note_pinned(
    app: AppHandle,
    window: Window,
    state: State<'_, AppState>,
    pinned: bool,
) -> Result<(), String> {
    let label = window.label().to_string();
    if let Some(webview) = app.get_webview_window(&label) {
        webview
            .set_always_on_top(pinned)
            .map_err(|e| format!("设置置顶失败：{e}"))?;
        // 取消置顶后要把窗口重新垫回"桌面之上、其它窗口之下"
        #[cfg(windows)]
        win_desktop_pin::set_user_pinned(&webview, pinned);
    }
    let id = require_note_id(&window)?;
    state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.pinned = pinned;
        }
    })
}

#[tauri::command]
async fn create_note(app: AppHandle) -> Result<String, String> {
    // 必须是 async：同步命令跑在主线程，而建窗口要等事件循环把 WebView2 创建完，
    // 主线程一阻塞就会死锁（表现为窗口建出来但是 about:blank）。
    let note = new_note_internal(&app)?;
    create_note_window(&app, &note, true).map_err(|e| format!("创建便签窗口失败：{e}"))?;
    Ok(note.id)
}

#[tauri::command]
fn delete_note(app: AppHandle, window: Window, state: State<'_, AppState>) -> Result<(), String> {
    let id = require_note_id(&window)?;
    let label = window.label().to_string();
    let remaining = state.update(move |s| {
        s.notes.retain(|n| n.id != id);
        s.notes.len()
    })?;

    if let Some(webview) = app.get_webview_window(&label) {
        let _ = webview.destroy();
    }

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
        .min_inner_size(240.0, 260.0)
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
    let _ = window.show();
    // 窗口是 visible:false 创建后再 show 的，Windows 上有一定概率把任务栏按钮带回来，
    // 这里显式再删一次（底层是 ITaskbarList::DeleteTab）
    let _ = window.set_skip_taskbar(true);
    if focus {
        let _ = window.set_focus();
    }

    attach_window_events(app, &window);

    // 抵抗 Win+D：把桌面窗口设成便签的 owner，再把便签垫到桌面之上、其它窗口之下
    #[cfg(windows)]
    win_desktop_pin::install(&window, note.pinned);

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
    // 最小化 / 隐藏时可能拿到 0，丢弃这次结果
    if size.width < 100 || size.height < 100 {
        return;
    }
    let Some(id) = note_id_from_label(label) else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let geometry = WindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let _ = state.update(move |s| {
        if let Some(note) = s.notes.iter_mut().find(|n| n.id == id) {
            note.bounds = Some(geometry);
        }
    });
}

// ---------------------------------------------------------------- 托盘

fn show_all_notes(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let notes = state.read(|s| s.notes.clone()).unwrap_or_default();
    let mut last = None;

    for note in &notes {
        if let Some(window) = app.get_webview_window(&note_label(&note.id)) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_skip_taskbar(true);
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

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let new_item = MenuItem::with_id(app, "new", "新建便签", true, None::<&str>)?;
    let show_item = MenuItem::with_id(app, "show_all", "显示全部便签", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide_all", "隐藏全部便签", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&new_item, &show_item, &hide_item, &quit_item],
    )?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("便签")
        .menu(&menu)
        // 左键单击唤回全部便签，右键才出菜单
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "new" => {
                // 托盘菜单回调在主线程，而建窗口需要事件循环继续跑，必须挪到后台线程
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    match new_note_internal(&app) {
                        Ok(note) => {
                            if let Err(err) = create_note_window(&app, &note, true) {
                                eprintln!("[tray] 新建便签窗口失败：{err}");
                            }
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
            _ => {}
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

// ---------------------------------------------------------------- entry

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let store_path = data_dir.join("store.json");

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
                geom_gen: Mutex::new(HashMap::new()),
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_note_state,
            save_note_content,
            set_note_title,
            set_note_style,
            set_note_pinned,
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
