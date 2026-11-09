//! rust-sticky-notes：无边框 + 透明背景 + 隐藏任务栏按钮的单便签应用。
//!
//! 设计要点：
//! - 数据全部放在一个 `store.json`（Tauri app data 目录），Rust 侧统一读写；
//! - 前端只负责编辑，所有落盘都经过这里的 debounce（编辑 450ms / 窗口 400ms）；
//! - 关窗只隐藏，进程常驻托盘；真正退出走托盘菜单或设置面板。

mod store;
#[cfg(windows)]
mod win_desktop_pin;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent, State, WebviewWindow,
    WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

use store::{NoteStyle, Store, TodoItem, WindowGeometry};

/// 主窗口标签，前后端统一用 "main"
const MAIN_WINDOW: &str = "main";

pub struct AppState {
    pub store: Mutex<Store>,
    pub path: std::path::PathBuf,
    /// 窗口几何变化的代次，用来做「安静后才能落盘」的 debounce
    pub persist_gen: AtomicU64,
}

impl AppState {
    fn persist(&self) -> Result<(), String> {
        let snapshot = {
            let guard = self.store.lock().map_err(|e| e.to_string())?;
            guard.clone()
        };
        store::save(&self.path, &snapshot)
    }

    fn with_store<T>(&self, f: impl FnOnce(&mut Store) -> T) -> Result<T, String> {
        let mut guard = self.store.lock().map_err(|e| e.to_string())?;
        Ok(f(&mut guard))
    }
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn load_state(state: State<'_, AppState>) -> Result<Store, String> {
    let guard = state.store.lock().map_err(|e| e.to_string())?;
    Ok(guard.clone())
}

#[tauri::command]
fn save_note(
    state: State<'_, AppState>,
    text: String,
    todos: Vec<TodoItem>,
) -> Result<(), String> {
    state.with_store(|s| {
        s.text = text;
        s.todos = todos;
    })?;
    state.persist()
}

#[tauri::command]
fn save_style(state: State<'_, AppState>, style: NoteStyle) -> Result<NoteStyle, String> {
    let normalized = style.normalized();
    state.with_store(|s| s.style = normalized.clone())?;
    state.persist()?;
    Ok(normalized)
}

#[tauri::command]
fn set_pinned(app: AppHandle, state: State<'_, AppState>, pinned: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window
            .set_always_on_top(pinned)
            .map_err(|e| format!("设置置顶失败：{e}"))?;
        // 取消置顶后要把窗口重新垫回"桌面之上、其它窗口之下"
        #[cfg(windows)]
        win_desktop_pin::set_user_pinned(&window, pinned);
    }
    state.with_store(|s| s.settings.always_on_top = pinned)?;
    state.persist()
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
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
    state.with_store(|s| s.settings.autostart = actual)?;
    state.persist()?;
    Ok(actual)
}

#[tauri::command]
fn get_store_path(state: State<'_, AppState>) -> String {
    state.path.display().to_string()
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    hide_main_window(&app);
}

#[tauri::command]
fn quit_app(app: AppHandle, state: State<'_, AppState>) {
    let _ = state.persist();
    app.exit(0);
}

// ---------------------------------------------------------------- window

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        // 先 unminimize 再 show，否则从 Win+D 的最小化状态唤回时不会还原
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
}

fn restore_geometry(window: &WebviewWindow, geometry: Option<&WindowGeometry>) {
    let Some(geo) = geometry else {
        let _ = window.center();
        return;
    };

    // 明显异常的数据直接丢弃（例如手动改坏了 JSON）
    if geo.width < 200 || geo.height < 200 {
        let _ = window.center();
        return;
    }

    let _ = window.set_size(PhysicalSize::new(geo.width, geo.height));

    if is_center_on_some_monitor(window, geo) {
        let _ = window.set_position(PhysicalPosition::new(geo.x, geo.y));
    } else {
        // 上次所在的显示器已经拔掉了，回到主屏中间
        let _ = window.center();
    }
}

/// 保存位置的中心点是否还落在某个显示器上（兼容副屏在左/上方导致的负坐标）
fn is_center_on_some_monitor(window: &WebviewWindow, geo: &WindowGeometry) -> bool {
    let Ok(monitors) = window.available_monitors() else {
        return false;
    };
    let center_x = geo.x + geo.width as i32 / 2;
    let center_y = geo.y + geo.height as i32 / 2;

    monitors.iter().any(|monitor| {
        let pos = monitor.position();
        let size = monitor.size();
        center_x >= pos.x
            && center_x <= pos.x + size.width as i32
            && center_y >= pos.y
            && center_y <= pos.y + size.height as i32
    })
}

fn attach_window_events(app: &AppHandle, window: &WebviewWindow) {
    let app = app.clone();
    let win = window.clone();

    window.on_window_event(move |event| match event {
        // 不用事件里带的坐标/尺寸：在 Windows 高 DPI 下，创建时的初始尺寸是逻辑像素，
        // 事件里的是物理像素，混着用会把窗口存小。统一在防抖后从窗口本身读一次。
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => schedule_persist(&app),
        WindowEvent::CloseRequested { api, .. } => {
            // 没有系统标题栏，这里只兜底：关窗 = 收进托盘
            api.prevent_close();
            let _ = win.hide();
        }
        _ => {}
    });
}

/// 从窗口读当前几何（物理像素）写进内存状态
fn capture_geometry(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    // 最小化 / 隐藏时可能拿到 0，丢弃这次结果
    if size.width < 100 || size.height < 100 {
        return;
    }
    let geometry = WindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };
    let _ = state.with_store(|s| s.window = Some(geometry));
}

/// 拖动/缩放会高频触发，等 400ms 没有新事件再写盘
fn schedule_persist(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let generation = state.persist_gen.fetch_add(1, Ordering::SeqCst) + 1;
    drop(state);

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if state.persist_gen.load(Ordering::SeqCst) != generation {
            return;
        }
        drop(state);
        capture_geometry(&app);
        if let Some(state) = app.try_state::<AppState>() {
            let _ = state.persist();
        }
    });
}

// ---------------------------------------------------------------- tray

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show_item = MenuItem::with_id(app, "show", "显示便签", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "隐藏便签", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("便签")
        .menu(&menu)
        // 左键单击唤回窗口，右键才出菜单
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "hide" => hide_main_window(app),
            "quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.persist();
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
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

            let mut data = store::load(&store_path);
            // 以系统真实的自启状态为准，避免 JSON 和注册表不一致
            data.settings.autostart = handle
                .autolaunch()
                .is_enabled()
                .unwrap_or(data.settings.autostart);

            let window = app
                .get_webview_window(MAIN_WINDOW)
                .expect("配置里缺少 label = main 的窗口");

            restore_geometry(&window, data.window.as_ref());
            let _ = window.set_always_on_top(data.settings.always_on_top);
            #[cfg(windows)]
            win_desktop_pin::set_user_pinned(&window, data.settings.always_on_top);
            let _ = window.show();
            // 窗口是 visible:false 创建后再 show 的，Windows 上有一定概率把任务栏按钮带回来，
            // 这里显式再删一次（底层是 ITaskbarList::DeleteTab）
            let _ = window.set_skip_taskbar(true);

            app.manage(AppState {
                store: Mutex::new(data),
                path: store_path,
                persist_gen: AtomicU64::new(0),
            });

            attach_window_events(&handle, &window);
            setup_tray(app)?;

            // 抵抗 Win+D：把桌面窗口设成便签的 owner，再把便签垫到桌面之上、其它窗口之下
            #[cfg(windows)]
            win_desktop_pin::install(&window);

            // 首次启动（或用户从没拖过窗口）也把当前几何落一次盘，保证下次能原样恢复
            capture_geometry(&handle);
            if let Some(state) = handle.try_state::<AppState>() {
                let _ = state.persist();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_state,
            save_note,
            save_style,
            set_pinned,
            get_autostart,
            set_autostart,
            get_store_path,
            hide_window,
            quit_app
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.persist();
                }
            }
        });
}
