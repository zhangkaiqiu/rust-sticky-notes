//! 窗口 / 数据状态快照与层级不变量断言（`--dump` / `--check` 的实现）。
//!
//! 目标：把"窗口层级对不对"从"人眼看截图"变成**机器可读的 JSON + 可断言的布尔值**。
//! 这里只**读**状态，绝不改层级——改层级统一走 `lib.rs` 里和 UI 相同的后端路径。
//!
//! 采集口径（脚本依赖这些约定，改之前先看 README「调试工具」）：
//! - `z` 来自 `EnumWindows` 的**全体可见顶层窗口**序号，0 = 最前；
//!   不在该枚举里的窗口（被隐藏 / 最小化到托盘）写 `null`；
//! - `exstyle` 是十六进制字符串，同时把 TOPMOST / NOACTIVATE / TOOLWINDOW 解成布尔字段；
//! - `owner_class` 用 `GetClassNameW` 取，用来确认 owner 仍是 `Progman`；
//! - 所有矩形都是**物理像素**（顶层 `"units": "physical"`）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::System::Console::{
    AttachConsole, FreeConsole, GetStdHandle, WriteConsoleW, ATTACH_PARENT_PROCESS,
    STD_OUTPUT_HANDLE,
};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetAwarenessFromDpiAwarenessContext,
    GetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
    GW_OWNER,
};

use crate::store::Note;
use crate::AppState;

/// `WS_EX_TOPMOST`：真正在置顶层
const EX_TOPMOST: isize = 0x0000_0008;
/// `WS_EX_TOOLWINDOW`：不占任务栏的辅助窗口
const EX_TOOLWINDOW: isize = 0x0000_0080;
/// `WS_EX_NOACTIVATE`：点击不抢焦点
const EX_NOACTIVATE: isize = 0x0800_0000;

/// 几何容差（物理像素）：`--check` 第 5 条允许的误差
const GEOMETRY_TOLERANCE: i32 = 2;

// ---------------------------------------------------------------- 数据结构

#[derive(Serialize, Debug, Clone)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct ClientRect {
    pub w: i32,
    pub h: i32,
}

/// store.json 里保存的几何（物理像素）
#[derive(Serialize, Debug, Clone)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Debug, Clone)]
pub struct Foreground {
    /// 前台窗口句柄；没有前台窗口时是 0
    pub hwnd: isize,
    pub class: String,
    pub title: String,
    /// 是否属于本进程（自己的便签窗口）
    pub is_ours: bool,
    /// 在 z 序里的序号；不在可见顶层窗口枚举里时为 null
    pub z: Option<usize>,
}

/// 数据侧的便签（字段刻意保持精简：脚本只断言这几个）
///
/// `visible` 是给第 6 条不变量用的：`visible=false` 的便签（启动时被隐藏、没建窗口）
/// 允许没有窗口，其余便签必须恰好有一个窗口。
#[derive(Serialize, Debug, Clone)]
pub struct NoteEntry {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub collapsed: bool,
    pub visible: bool,
    pub bounds: Option<Bounds>,
}

/// 窗口侧的一条记录
#[derive(Serialize, Debug, Clone)]
pub struct WindowEntry {
    pub label: String,
    pub note_id: String,
    pub hwnd: isize,
    pub exstyle: String,
    pub topmost: bool,
    pub noactivate: bool,
    pub toolwindow: bool,
    pub owner_hwnd: isize,
    pub owner_class: String,
    pub class: String,
    pub title: String,
    pub visible: bool,
    pub iconic: bool,
    pub focused: bool,
    pub rect: Rect,
    pub client: ClientRect,
    pub z: Option<usize>,
    pub z_total: usize,
}

/// 一份完整快照：`--dump` 原样写进 JSON，`--check` 用它做断言
#[derive(Serialize, Debug, Clone)]
pub struct Snapshot {
    pub timestamp: String,
    pub pid: u32,
    pub dpi_awareness: String,
    pub store_path: String,
    pub portable: bool,
    /// 矩形单位；恒为 "physical"
    pub units: &'static str,
    pub foreground: Foreground,
    pub notes: Vec<NoteEntry>,
    pub windows: Vec<WindowEntry>,
    /// 失败的断言摘要（`--check` 的 violations 与它同源）
    pub violations: Vec<String>,
}

/// 单条不变量的结果
#[derive(Serialize, Debug, Clone)]
pub struct CheckItem {
    /// 稳定 slug，脚本按它取结果
    pub id: &'static str,
    pub name: &'static str,
    /// "pass" | "fail" | "skip"
    pub status: &'static str,
    pub detail: String,
}

/// `--check` 的执行结果
pub struct CheckOutcome {
    pub ok: bool,
    pub passed: usize,
    pub total: usize,
    pub checks: Vec<CheckItem>,
}

// ---------------------------------------------------------------- 采集

/// 采集一份快照（含 violations）。只读，可以在主线程直接调。
pub fn collect(app: &AppHandle) -> Snapshot {
    let (store_path, portable, notes) = match app.try_state::<AppState>() {
        Some(state) => {
            let path = state.path.display().to_string();
            let notes = state.read(|s| s.notes.clone()).unwrap_or_default();
            (path, state.portable, notes)
        }
        None => (String::new(), false, Vec::new()),
    };

    let zorder = top_level_windows();
    let our_pid = std::process::id();
    let fg = unsafe { GetForegroundWindow() };

    // 便签窗口：只认 label 前缀，避免把托盘 / 单实例事件窗混进来
    let mut windows: Vec<WindowEntry> = Vec::new();
    for (label, window) in app.webview_windows() {
        let Some(note_id) = label.strip_prefix(crate::NOTE_LABEL_PREFIX) else {
            continue;
        };
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        windows.push(build_window_entry(&label, note_id, raw, &zorder, fg));
    }
    // 按 z 排序（0 = 最前），隐藏窗口排最后：脚本可以直接取第一条当"最前面那张"
    windows.sort_by(|a, b| match (a.z, b.z) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.label.cmp(&b.label)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.label.cmp(&b.label),
    });

    let fg_is_ours = !fg.is_invalid() && process_id_of(fg) == our_pid;

    let mut snapshot = Snapshot {
        timestamp: timestamp(),
        pid: our_pid,
        dpi_awareness: dpi_awareness(),
        store_path,
        portable,
        units: "physical",
        foreground: Foreground {
            hwnd: fg.0 as isize,
            class: class_name(fg),
            title: window_title(fg),
            is_ours: fg_is_ours,
            z: zorder.iter().position(|h| *h == fg),
        },
        notes: notes.iter().map(note_entry).collect(),
        windows,
        violations: Vec::new(),
    };

    snapshot.violations = evaluate(&snapshot)
        .into_iter()
        .filter(|item| item.status == "fail")
        .map(|item| format!("{}：{}", item.name, item.detail))
        .collect();
    snapshot
}

fn note_entry(note: &Note) -> NoteEntry {
    NoteEntry {
        id: note.id.clone(),
        title: note.title.clone(),
        pinned: note.pinned,
        collapsed: note.collapsed,
        visible: note.visible,
        bounds: note.bounds.as_ref().map(|b| Bounds {
            x: b.x,
            y: b.y,
            width: b.width,
            height: b.height,
        }),
    }
}

fn build_window_entry(
    label: &str,
    note_id: &str,
    hwnd: HWND,
    zorder: &[HWND],
    fg: HWND,
) -> WindowEntry {
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        rect = RECT::default();
    }
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut client) };

    WindowEntry {
        label: label.to_string(),
        note_id: note_id.to_string(),
        hwnd: hwnd.0 as isize,
        exstyle: format!("0x{ex:X}"),
        topmost: ex & EX_TOPMOST != 0,
        noactivate: ex & EX_NOACTIVATE != 0,
        toolwindow: ex & EX_TOOLWINDOW != 0,
        owner_hwnd: owner.0 as isize,
        owner_class: class_name(owner),
        class: class_name(hwnd),
        title: window_title(hwnd),
        visible: unsafe { IsWindowVisible(hwnd).as_bool() },
        iconic: unsafe { IsIconic(hwnd).as_bool() },
        focused: hwnd == fg,
        rect: Rect {
            x: rect.left,
            y: rect.top,
            w: rect.right - rect.left,
            h: rect.bottom - rect.top,
        },
        client: ClientRect {
            w: client.right - client.left,
            h: client.bottom - client.top,
        },
        z: zorder.iter().position(|h| *h == hwnd),
        z_total: zorder.len(),
    }
}

/// 全体可见顶层窗口，`EnumWindows` 顺序（0 = 最前）
fn top_level_windows() -> Vec<HWND> {
    struct Scan {
        found: Vec<isize>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Scan);
        if IsWindowVisible(hwnd).as_bool() {
            ctx.found.push(hwnd.0 as isize);
        }
        true.into()
    }

    let mut ctx = Scan { found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut ctx as *mut Scan as isize));
    }
    ctx.found
        .into_iter()
        .map(|v| HWND(v as *mut core::ffi::c_void))
        .collect()
}

fn class_name(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return String::new();
    }
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

fn window_title(hwnd: HWND) -> String {
    if hwnd.is_invalid() {
        return String::new();
    }
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

fn process_id_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    pid
}

/// DPI 感知级别：Tauri 2 在 Windows 上用的是 per-monitor-v2
fn dpi_awareness() -> String {
    unsafe {
        let ctx = GetThreadDpiAwarenessContext();
        if AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).as_bool() {
            return "per-monitor-v2".to_string();
        }
        match GetAwarenessFromDpiAwarenessContext(ctx).0 {
            0 => "unaware".to_string(),
            1 => "system".to_string(),
            2 => "per-monitor".to_string(),
            _ => "unknown".to_string(),
        }
    }
}

/// 本地时间 + 时区偏移，形如 `2026-11-11T20:00:00+08:00`
fn timestamp() -> String {
    use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};

    let local = unsafe { GetLocalTime() };
    let utc = unsafe { GetSystemTime() };

    let local_min = minutes_since_epoch(
        local.wYear as i64,
        local.wMonth as i64,
        local.wDay as i64,
        local.wHour as i64,
        local.wMinute as i64,
    );
    let utc_min = minutes_since_epoch(
        utc.wYear as i64,
        utc.wMonth as i64,
        utc.wDay as i64,
        utc.wHour as i64,
        utc.wMinute as i64,
    );

    // 偏移只会落在 [-12h, +14h]，把跨日的 1440 分钟折回来
    let mut offset = local_min - utc_min;
    while offset > 14 * 60 {
        offset -= 24 * 60;
    }
    while offset < -12 * 60 {
        offset += 24 * 60;
    }

    let sign = if offset < 0 { '-' } else { '+' };
    let abs = offset.abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
        local.wYear,
        local.wMonth,
        local.wDay,
        local.wHour,
        local.wMinute,
        local.wSecond,
        sign,
        abs / 60,
        abs % 60
    )
}

/// 某一时刻距 1970-01-01 的分钟数（本地墙钟，不涉及时区）
fn minutes_since_epoch(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> i64 {
    // Howard Hinnant 的 days_from_civil：公历 → 儒略日天数
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 1440 + hour * 60 + minute
}

// ---------------------------------------------------------------- 不变量

/// 6 条层级不变量。任何一条 fail 都会进 `violations`。
pub fn evaluate(snapshot: &Snapshot) -> Vec<CheckItem> {
    vec![
        check_pin_consistency(snapshot),
        check_owner_progman(snapshot),
        check_no_cover_foreground(snapshot),
        check_pinned_on_top(snapshot),
        check_geometry(snapshot),
        check_window_data_match(snapshot),
    ]
}

/// 按 id 取便签
fn note_of<'a>(snapshot: &'a Snapshot, id: &str) -> Option<&'a NoteEntry> {
    snapshot.notes.iter().find(|note| note.id == id)
}

/// 1. 置顶一致：数据里的 pinned 必须等于窗口上真实的 `WS_EX_TOPMOST`
fn check_pin_consistency(snapshot: &Snapshot) -> CheckItem {
    let mut bad = Vec::new();
    let mut checked = 0;

    for window in &snapshot.windows {
        let Some(note) = note_of(snapshot, &window.note_id) else {
            continue;
        };
        checked += 1;
        if note.pinned != window.topmost {
            bad.push(format!(
                "{} 数据 pinned={} 但窗口 topmost={}（exstyle={}）",
                window.label, note.pinned, window.topmost, window.exstyle
            ));
        }
    }

    if !bad.is_empty() {
        return fail("pin_consistency", "置顶一致", bad.join("；"));
    }
    CheckItem {
        id: "pin_consistency",
        name: "置顶一致",
        status: if checked == 0 { "skip" } else { "pass" },
        detail: if checked == 0 {
            "没有可校验的便签窗口".to_string()
        } else {
            format!("{checked} 张便签的 pinned 与 WS_EX_TOPMOST 一致")
        },
    }
}

/// 2. owner 正确：owner 必须是 Progman（Win+D 抗性的前提）
fn check_owner_progman(snapshot: &Snapshot) -> CheckItem {
    let mut bad = Vec::new();

    for window in &snapshot.windows {
        if window.owner_class != "Progman" {
            bad.push(format!(
                "{} owner_hwnd={} owner_class=\"{}\"（期望 Progman）",
                window.label, window.owner_hwnd, window.owner_class
            ));
        }
    }

    if !bad.is_empty() {
        return fail("owner_progman", "owner 正确", bad.join("；"));
    }
    CheckItem {
        id: "owner_progman",
        name: "owner 正确",
        status: if snapshot.windows.is_empty() {
            "skip"
        } else {
            "pass"
        },
        detail: format!("{} 个便签窗口的 owner 均为 Progman", snapshot.windows.len()),
    }
}

/// 3. 不越界：**未置顶便签不得压住前台窗口**
///
/// 例外只有一个：这张窗口自己就是前台窗口（用户刚点它，它是前台，不是"压住前台"）。
/// 前台窗口是我们自己的便签 / 拿不到前台窗口时整条跳过——此时这条断言没有意义。
fn check_no_cover_foreground(snapshot: &Snapshot) -> CheckItem {
    let fg = &snapshot.foreground;
    if fg.hwnd == 0 {
        return skip("no_cover_foreground", "不越界", "没有前台窗口，跳过");
    }
    if fg.is_ours {
        return skip(
            "no_cover_foreground",
            "不越界",
            "前台是本进程的窗口（便签被激活），本条不适用",
        );
    }
    let Some(fg_z) = fg.z else {
        return skip(
            "no_cover_foreground",
            "不越界",
            format!(
                "前台窗口 {} 不在可见顶层窗口枚举里，无法比较 z",
                fg.class
            ),
        );
    };

    let mut bad = Vec::new();
    let mut checked = Vec::new();
    for window in &snapshot.windows {
        let Some(note) = note_of(snapshot, &window.note_id) else {
            continue;
        };
        if note.pinned || !window.visible {
            continue;
        }
        match window.z {
            // 在前台之下：正常
            Some(z) if z > fg_z => checked.push(format!("{} z={z}", window.label)),
            // 压住了前台：违约
            Some(z) => bad.push(format!(
                "{} 未置顶但 z={z} 比前台窗口 z={fg_z} 更靠前（会盖住前台应用）",
                window.label
            )),
            None => {} // 隐藏窗口没有 z 可言
        }
    }

    if !bad.is_empty() {
        return fail("no_cover_foreground", "不越界", bad.join("；"));
    }
    CheckItem {
        id: "no_cover_foreground",
        name: "不越界",
        status: if checked.is_empty() { "skip" } else { "pass" },
        detail: if checked.is_empty() {
            "没有可见的未置顶便签窗口".to_string()
        } else {
            format!(
                "未置顶便签 {} 均在前台窗口（{} z={fg_z}）之下",
                checked.join("、"),
                fg.class
            )
        },
    }
}

/// 4. 置顶在最前：所有置顶窗口的 z 都小于所有未置顶窗口的 z
fn check_pinned_on_top(snapshot: &Snapshot) -> CheckItem {
    let mut pinned = Vec::new();
    let mut unpinned = Vec::new();

    for window in &snapshot.windows {
        let Some(note) = note_of(snapshot, &window.note_id) else {
            continue;
        };
        let Some(z) = window.z else {
            continue;
        };
        if note.pinned {
            pinned.push((z, window.label.clone()));
        } else {
            unpinned.push((z, window.label.clone()));
        }
    }

    if pinned.is_empty() || unpinned.is_empty() {
        return skip(
            "pinned_on_top",
            "置顶在最前",
            format!(
                "只有{}置顶便签窗口，无法比较两段 z 序",
                if pinned.is_empty() { "未" } else { "" }
            ),
        );
    }

    let max_pinned = pinned.iter().max_by_key(|(z, _)| *z).unwrap().clone();
    let min_unpinned = unpinned.iter().min_by_key(|(z, _)| *z).unwrap().clone();
    if max_pinned.0 < min_unpinned.0 {
        CheckItem {
            id: "pinned_on_top",
            name: "置顶在最前",
            status: "pass",
            detail: format!(
                "置顶最靠后 z={}（{}）< 未置顶最靠前 z={}（{}）",
                max_pinned.0, max_pinned.1, min_unpinned.0, min_unpinned.1
            ),
        }
    } else {
        fail(
            "pinned_on_top",
            "置顶在最前",
            format!(
                "置顶窗口 {} z={} 落在未置顶窗口 {} z={} 之后",
                max_pinned.1, max_pinned.0, min_unpinned.1, min_unpinned.0
            ),
        )
    }
}

/// 5. 几何一致：展开态窗口的 rect 宽高必须等于 bounds 宽高（容差 2px，折叠态跳过）
fn check_geometry(snapshot: &Snapshot) -> CheckItem {
    let mut bad = Vec::new();
    let mut checked = 0;
    let mut missing = Vec::new();

    for window in &snapshot.windows {
        let Some(note) = note_of(snapshot, &window.note_id) else {
            continue;
        };
        if note.collapsed {
            continue;
        }
        let Some(bounds) = note.bounds.as_ref() else {
            missing.push(window.label.clone());
            continue;
        };
        checked += 1;
        let dw = (window.rect.w - bounds.width as i32).abs();
        let dh = (window.rect.h - bounds.height as i32).abs();
        if dw > GEOMETRY_TOLERANCE || dh > GEOMETRY_TOLERANCE {
            bad.push(format!(
                "{} rect={}x{} 与 bounds={}x{} 相差超过 {}px",
                window.label, window.rect.w, window.rect.h, bounds.width, bounds.height, GEOMETRY_TOLERANCE
            ));
        }
    }

    if !bad.is_empty() {
        return fail("geometry_match", "几何一致", bad.join("；"));
    }
    if checked == 0 {
        return skip("geometry_match", "几何一致", "没有展开态便签窗口可比");
    }
    let mut detail = format!("{checked} 张展开态便签的 rect 与 bounds 一致（±{GEOMETRY_TOLERANCE}px）");
    if !missing.is_empty() {
        detail.push_str(&format!("；{} 无 bounds 已跳过", missing.join("、")));
    }
    CheckItem {
        id: "geometry_match",
        name: "几何一致",
        status: "pass",
        detail,
    }
}

/// 6. 窗口与数据对得上：每个 id 恰好一个窗口，不多不少
///
/// 注意 `visible=false` 的便签允许没有窗口（启动时被隐藏、没建窗口），
/// 但它们也不能凭空多出窗口来；窗口指向未知便签一律算错。
fn check_window_data_match(snapshot: &Snapshot) -> CheckItem {
    let mut bad = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();

    for window in &snapshot.windows {
        *counts.entry(window.note_id.as_str()).or_insert(0) += 1;
        if note_of(snapshot, &window.note_id).is_none() {
            bad.push(format!(
                "窗口 {} 指向的便签 {} 不在数据里",
                window.label, window.note_id
            ));
        }
    }
    for (id, count) in &counts {
        if *count > 1 {
            bad.push(format!("便签 {id} 对应 {count} 个窗口（应恰好 1 个）"));
        }
    }
    for note in &snapshot.notes {
        if note.visible && !counts.contains_key(note.id.as_str()) {
            bad.push(format!("便签 {}（{}）可见但没有窗口", note.id, note.title));
        }
    }

    if !bad.is_empty() {
        return fail("window_data_match", "窗口与数据对得上", bad.join("；"));
    }
    CheckItem {
        id: "window_data_match",
        name: "窗口与数据对得上",
        status: if snapshot.notes.is_empty() {
            "skip"
        } else {
            "pass"
        },
        detail: format!(
            "{} 张便签对应 {} 个窗口，id 一一对应",
            snapshot.notes.len(),
            snapshot.windows.len()
        ),
    }
}

fn fail(id: &'static str, name: &'static str, detail: impl Into<String>) -> CheckItem {
    CheckItem {
        id,
        name,
        status: "fail",
        detail: detail.into(),
    }
}

fn skip(id: &'static str, name: &'static str, detail: impl Into<String>) -> CheckItem {
    CheckItem {
        id,
        name,
        status: "skip",
        detail: detail.into(),
    }
}

// ---------------------------------------------------------------- 输出

/// `--dump` 的默认路径：`%TEMP%\sticky-dump.json`
pub fn default_dump_path() -> PathBuf {
    std::env::temp_dir().join("sticky-dump.json")
}

/// `--check` 的两份输出：人读 txt + 机器读 json
pub fn check_paths() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir();
    (dir.join("sticky-check.txt"), dir.join("sticky-check.json"))
}

/// 写 dump 文件（目录不存在会自动创建）
pub fn write_dump(snapshot: &Snapshot, out: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    std::fs::write(out, json).map_err(|e| format!("写入 {} 失败：{e}", out.display()))
}

/// 执行断言并写两份结果文件（同时把这份数据 dump 出来，方便失败时直接贴 JSON）
pub fn run_check(snapshot: &Snapshot, dump_path: &Path) -> Result<CheckOutcome, String> {
    let checks = evaluate(snapshot);
    write_dump(snapshot, dump_path)?;

    let total = checks.len();
    let passed = checks.iter().filter(|item| item.status != "fail").count();
    let ok = passed == total;
    let violations: Vec<String> = checks
        .iter()
        .filter(|item| item.status == "fail")
        .map(|item| format!("{}：{}", item.name, item.detail))
        .collect();

    let (txt_path, json_path) = check_paths();

    // 人读版本
    let mut text = String::new();
    text.push_str("rust-sticky-notes 窗口层级不变量检查\n");
    text.push_str(&format!("时间：{}\n", snapshot.timestamp));
    text.push_str(&format!("进程：{}\n", snapshot.pid));
    text.push_str(&format!("数据：{}\n", snapshot.store_path));
    text.push_str(&format!(
        "前台：hwnd={} class=\"{}\" title=\"{}\" is_ours={} z={}\n",
        snapshot.foreground.hwnd,
        snapshot.foreground.class,
        snapshot.foreground.title,
        snapshot.foreground.is_ours,
        snapshot
            .foreground
            .z
            .map(|z| z.to_string())
            .unwrap_or_else(|| "null".to_string())
    ));
    text.push_str(&format!("dump：{}\n\n", dump_path.display()));
    for (index, item) in checks.iter().enumerate() {
        text.push_str(&format!(
            "[{}] {}/{} {}：{}\n",
            item.status.to_uppercase(),
            index + 1,
            total,
            item.name,
            item.detail
        ));
    }
    text.push_str("\n");
    if ok {
        text.push_str(&format!("PASS {passed}/{total}\n"));
    } else {
        text.push_str(&format!("FAIL: {}\n", violations.join("；")));
    }
    std::fs::write(&txt_path, text).map_err(|e| format!("写入 {} 失败：{e}", txt_path.display()))?;

    // 机器读版本
    let json = serde_json::json!({
        "timestamp": snapshot.timestamp,
        "pid": snapshot.pid,
        "ok": ok,
        "passed": passed,
        "total": total,
        "store_path": snapshot.store_path,
        "dump_path": dump_path.display().to_string(),
        "checks": checks
            .iter()
            .map(|item| serde_json::json!({
                "id": item.id,
                "name": item.name,
                "status": item.status,
                "detail": item.detail,
            }))
            .collect::<Vec<_>>(),
        "violations": violations,
    });
    let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&json_path, pretty)
        .map_err(|e| format!("写入 {} 失败：{e}", json_path.display()))?;

    Ok(CheckOutcome {
        ok,
        passed,
        total,
        checks,
    })
}

/// 尽力附到父进程的控制台打印一行提示（GUI 子系统默认没有控制台）。
///
/// 附不上就**静默**返回——所有脚本判定的依据是文件内容，不依赖这行输出。
/// 注意：只有"命令由当前进程自己执行"时（没有已运行实例、本次启动即执行）才可能附上；
/// 单实例转发时命令跑在已经脱离终端的旧进程里，这里会安静失败。
pub fn console_notice(line: &str) {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return;
        }
        if let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) {
            let wide: Vec<u16> = line.encode_utf16().collect();
            let _ = WriteConsoleW(handle, &wide, None, None);
        }
        let _ = FreeConsole();
    }
}
