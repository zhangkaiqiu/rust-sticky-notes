//! 抵抗 Win+D（显示桌面）。
//!
//! ## 现象与机制
//! Win+D 会把普通窗口最小化/停放到 `-32000`，并把桌面窗口 `Progman` 抬到普通窗口之上。
//! 便签因为 `skipTaskbar` 不在 shell 的窗口枚举里，本来就不会被最小化，只是被抬起的
//! Progman 盖住。所以只要在"显示桌面"期间让便签浮在 Progman 之上就够了。
//!
//! ## 做法
//! `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 监听前台变化：
//! - 前台变成桌面窗口 → 判定是否真的在"显示桌面"（见 `desktop_is_shown`），是则临时置顶；
//! - 前台变成别的应用 → 撤销临时置顶（用户自己按图钉置顶的不动）。
//!
//! 判定不能用"前台是不是 Progman"：explorer 刚重启、用户点桌面空白处、或没有窗口有焦点时，
//! 前台同样是 Progman，但那时应用窗口还在，便签不该浮起来。这里用两个独立信号取或：
//! 1. Progman 被抬到了普通层最上面（正常显示桌面时 shell 正是这么干的）；
//! 2. 屏幕上没有任何"正常显示着"的应用窗口（其余都被最小化/停放到屏幕外了）。
//!
//! ## 为什么不用 owner = Progman
//! 更"优雅"（不涉及置顶），但实测会把 shell 的显示桌面状态机顶死：第一次 Win+D 正常，
//! 第二次不再恢复之前的窗口。副作用不可接受，已放弃。
//!
//! ## 为什么不用 tauri-plugin-wallpaper 的 pin
//! 它把 `HWND_TOPMOST` 写死在每次 `WM_WINDOWPOSCHANGING` 里，等于永久置顶，破坏
//! "便签会被前台应用正常覆盖"的要求。
//!
//! ## 附带兜底
//! `WM_WINDOWPOSCHANGING` 里拦截 `-32000` 停放：个别系统上 Show Desktop 会直接挪窗口，
//! 此时窗口位置会被改掉，临时置顶也救不回来。窗口子类化只处理这一条，其它消息原样透传。

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect,
    IsIconic, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, EVENT_SYSTEM_FOREGROUND,
    GWL_EXSTYLE, GWLP_WNDPROC, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, WM_WINDOWPOSCHANGING, WINDOWPOS, WINEVENT_OUTOFCONTEXT, WNDPROC,
};

/// Show Desktop 把窗口"停放"到屏幕外时用的坐标
const OFFSCREEN_PARK: i32 = -32000;
/// WS_EX_TOPMOST
const WS_EX_TOPMOST: u32 = 0x8;
/// 太小的辅助窗口不算"应用窗口"
const MIN_WINDOW_EDGE: i32 = 200;
/// 被停放/移出屏幕的窗口坐标阈值
const OFFSCREEN_LIMIT: i32 = -30000;

static NOTE_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
/// 用户按图钉手动置顶的状态
static USER_PINNED: AtomicBool = AtomicBool::new(false);
/// 当前置顶是不是为了 Win+D 临时加的
static AUTO_TOP: AtomicBool = AtomicBool::new(false);

/// 给主窗口装上"抵抗 Win+D"，setup 里调一次。
pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(hwnd) = window.hwnd() else {
        eprintln!("[win-d] 取窗口句柄失败，跳过 Win+D 防护");
        return;
    };
    NOTE_HWND.store(hwnd.0 as isize, Ordering::Release);

    unsafe {
        let original = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wndproc as *const () as usize as isize);
        if original == 0 {
            eprintln!("[win-d] 子类化失败，-32000 兜底不可用（主功能不受影响）");
        } else {
            ORIGINAL_WNDPROC.store(original, Ordering::Release);
        }

        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if hook.is_invalid() {
            eprintln!("[win-d] SetWinEventHook 失败，Win+D 防护不可用");
            debug_log("SetWinEventHook 失败");
        } else {
            debug_log("已安装 EVENT_SYSTEM_FOREGROUND 钩子");
        }
    }
}

/// 用户按图钉切换置顶时同步状态，避免临时置顶和用户置顶打架
pub fn set_user_pinned(pinned: bool) {
    USER_PINNED.store(pinned, Ordering::Release);
    if pinned {
        AUTO_TOP.store(false, Ordering::Release);
    }
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let note = NOTE_HWND.load(Ordering::Acquire);
    if note == 0 || hwnd.0.is_null() {
        return;
    }
    let note_hwnd = HWND(note as *mut core::ffi::c_void);
    let class = class_of(hwnd);

    // 1) 前台变成桌面窗口
    if class == "Progman" || class == "WorkerW" {
        if USER_PINNED.load(Ordering::Acquire) {
            return;
        }
        let shown = desktop_is_shown();
        debug_log(&format!("前台=桌面 显示中={shown}"));
        if shown {
            request_topmost(note, true);
            AUTO_TOP.store(true, Ordering::Release);
        } else {
            // shell 抬桌面可能比前台切换晚一点，延迟复查
            schedule_delayed_check();
        }
        return;
    }

    // 2) 焦点回到便签：可能桌面还显示着（用户点了便签），也可能桌面已恢复、焦点落回便签。
    if hwnd == note_hwnd {
        if !desktop_is_shown() && AUTO_TOP.swap(false, Ordering::AcqRel) {
            debug_log("焦点回到便签且桌面已恢复 → 撤销临时置顶");
            request_topmost(note, false);
        }
        return;
    }

    // 3) 前台变成别的应用 → 退出显示桌面
    if AUTO_TOP.swap(false, Ordering::AcqRel) {
        debug_log("前台切到其他应用 → 撤销临时置顶");
        request_topmost(note, false);
    }
}

/// 临时置顶请求的代次：任何新请求（置顶/撤销）都会让旧的重试循环失效
static TOPMOST_GEN: AtomicU64 = AtomicU64::new(0);
/// 置顶被系统吞掉时的重试间隔与次数（覆盖 ~1.8s 的过渡窗口）
const TOPMOST_RETRY_MS: u64 = 120;
const TOPMOST_RETRY_MAX: u32 = 15;

fn apply_topmost(hwnd: HWND, on: bool) {
    unsafe {
        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        let before = current_exstyle(hwnd) & WS_EX_TOPMOST != 0;
        let result = SetWindowPos(
            hwnd,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
        let after_state = current_exstyle(hwnd) & WS_EX_TOPMOST != 0;
        debug_log(&format!(
            "apply_topmost({on}) 结果={:?} 改前置顶={before} 改后置顶={after_state}",
            result.is_ok()
        ));
    }
}

unsafe fn current_exstyle(hwnd: HWND) -> u32 {
    GetWindowLongW(hwnd, GWL_EXSTYLE) as u32
}

unsafe fn is_topmost(hwnd: HWND) -> bool {
    current_exstyle(hwnd) & WS_EX_TOPMOST != 0
}

/// 置顶/撤销置顶的统一入口。
///
/// 实测：Win+D 刚触发的那一小段时间里，`SetWindowPos(HWND_TOPMOST)` 会返回成功但
/// `WS_EX_TOPMOST` 位不变（被 shell 的过渡状态吞掉），过几百毫秒后同样的调用就生效。
/// 单发调用因此时灵时不灵，这里改成"立即试一次 + 未生效则短间隔重试"，并用代次号
/// 保证撤销请求能立刻终止还在跑的重试循环。
fn request_topmost(note_raw: isize, on: bool) {
    let gen = TOPMOST_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    if note_raw == 0 {
        return;
    }

    if !on {
        apply_topmost(raw_hwnd(note_raw), false);
        return;
    }

    apply_topmost(raw_hwnd(note_raw), true);
    if unsafe { is_topmost(raw_hwnd(note_raw)) } {
        return;
    }

    std::thread::spawn(move || {
        for attempt in 1..=TOPMOST_RETRY_MAX {
            if TOPMOST_GEN.load(Ordering::Acquire) != gen || USER_PINNED.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(TOPMOST_RETRY_MS));
            if TOPMOST_GEN.load(Ordering::Acquire) != gen || USER_PINNED.load(Ordering::Acquire) {
                return;
            }
            apply_topmost(raw_hwnd(note_raw), true);
            if unsafe { is_topmost(raw_hwnd(note_raw)) } {
                debug_log(&format!("置顶在第 {attempt} 次重试后生效"));
                return;
            }
        }
        debug_log("置顶重试全部失败");
    });
}

fn raw_hwnd(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

/// shell 抬桌面/最小化可能比前台切换晚几毫秒，250ms 后复查一次
fn schedule_delayed_check() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let note = NOTE_HWND.load(Ordering::Acquire);
        if note == 0 || USER_PINNED.load(Ordering::Acquire) {
            return;
        }
        let shown = desktop_is_shown();
        debug_log(&format!("延迟复查 显示中={shown}"));
        if !shown {
            return;
        }
        request_topmost(note, true);
        AUTO_TOP.store(true, Ordering::Release);
    });
}

fn desktop_is_shown() -> bool {
    progman_raised() || !any_app_window_on_screen()
}

unsafe fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = GetClassNameW(hwnd, &mut buf);
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

struct RaisedScan {
    note: isize,
    raised: bool,
}

unsafe extern "system" fn scan_normal_band_top(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let ctx = &mut *(lparam.0 as *mut RaisedScan);

    if !IsWindowVisible(hwnd).as_bool() || hwnd.0 as isize == ctx.note {
        return true.into();
    }
    // topmost 窗口（托盘、任务栏、临时置顶的便签）不参与普通层判断
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 {
        return true.into();
    }
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err()
        || (rect.right - rect.left) < MIN_WINDOW_EDGE
        || (rect.bottom - rect.top) < MIN_WINDOW_EDGE
    {
        return true.into();
    }

    // EnumWindows 按 Z 序从前往后枚举：第一个够大的普通窗口就是普通层最上面那个。
    // 它是桌面窗口 → 说明桌面被抬起来了。
    let class = class_of(hwnd);
    ctx.raised = class == "Progman" || class == "WorkerW";
    false.into()
}

fn progman_raised() -> bool {
    let mut ctx = RaisedScan {
        note: NOTE_HWND.load(Ordering::Acquire),
        raised: false,
    };
    unsafe {
        let _ = EnumWindows(
            Some(scan_normal_band_top),
            LPARAM(&mut ctx as *mut RaisedScan as isize),
        );
    }
    ctx.raised
}

struct AppScan {
    note: isize,
    found: bool,
}

unsafe extern "system" fn scan_app_window(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let ctx = &mut *(lparam.0 as *mut AppScan);

    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return true.into();
    }
    if hwnd.0 as isize == ctx.note {
        return true.into();
    }

    let class = class_of(hwnd);
    // 桌面/任务栏/UWP 宿主这类系统窗口不算"应用"
    if matches!(
        class.as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "ApplicationFrameWindow"
            | "WinUIDesktopWin32WindowClass"
            | "CEF-OSC-WIDGET"
    ) {
        return true.into();
    }

    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_err() {
        return true.into();
    }
    if (rect.right - rect.left) < MIN_WINDOW_EDGE || (rect.bottom - rect.top) < MIN_WINDOW_EDGE {
        return true.into();
    }
    // 被 Show Desktop 停放到屏幕外的窗口不算"还显示着"
    if rect.left <= OFFSCREEN_LIMIT || rect.top <= OFFSCREEN_LIMIT {
        return true.into();
    }

    ctx.found = true;
    false.into()
}

/// 屏幕上是否还有"正常显示着"的应用窗口
fn any_app_window_on_screen() -> bool {
    let mut ctx = AppScan {
        note: NOTE_HWND.load(Ordering::Acquire),
        found: false,
    };
    unsafe {
        let _ = EnumWindows(
            Some(scan_app_window),
            LPARAM(&mut ctx as *mut AppScan as isize),
        );
    }
    ctx.found
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_WINDOWPOSCHANGING {
        let pos = lparam.0 as *mut WINDOWPOS;
        if !pos.is_null() {
            let pos = &mut *pos;
            if pos.x == OFFSCREEN_PARK && pos.y == OFFSCREEN_PARK {
                pos.flags |= SWP_NOMOVE | SWP_NOSIZE;
            }
        }
    }

    let original = ORIGINAL_WNDPROC.load(Ordering::Acquire);
    if original != 0 {
        let original: WNDPROC = std::mem::transmute::<isize, WNDPROC>(original);
        return CallWindowProcW(original, hwnd, msg, wparam, lparam);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// 调试日志：`%TEMP%\rust-sticky-notes-win-d.log`
fn debug_log(message: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("rust-sticky-notes-win-d.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{} {}", timestamp(), message);
    }
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}
