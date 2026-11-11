//! 便签的桌面层级：抗 Win+D + 图钉 + 「操作 A 不得改变 B」。
//!
//! # 抗 Win+D：owner = Progman
//!
//! `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 把桌面窗口设成便签的 owner。
//! owner 窗口永远在它的 owner 之上，所以 Win+D 抬起桌面时便签跟着浮在桌面上，
//! 全程不需要置顶切换（也就没有置顶/取消置顶带来的重绘跳动）。
//!
//! # 代价：owner 相同 = 同一个 z-order 组
//!
//! Windows 把 owner 相同的窗口当成一组，**激活组内任意一张会把整组抬到普通层最前**。
//! 实测：A 置顶、B 沉底（z=25）、记事本在前台（z=4），仅激活 A，B 就被搬到 z=4 并画出来。
//! 这个连带由系统发起、不经过我们的代码，事后补垫只能缩短抖动，用户看到的是"闪一下"。
//!
//! 试过的两条路都不行：
//!
//! - **事后补垫**：抖动只是被缩短，B 仍然先被画到前面再被压回去；
//! - **给每条便签各挂一个屏幕外影子窗口做 owner**：影子自身又同属 `Progman`，
//!   组只是往下挪了一层，激活 A 照样把 B 的整条链抬起来（实测 B 仍然 27 → 5）。
//!
//! # 真正有效的做法：在系统改 z 序的那一刻拦掉
//!
//! 给便签窗口装一个窗口过程钩子，拦 `WM_WINDOWPOSCHANGING`——它在系统真正重排并重绘
//! **之前**到达。当这次改 z 序不是这张便签自己的交互引起时，直接给它加上 `SWP_NOZORDER`：
//! 该窗口的 z 序**原封不动**，既不上升也不下沉，更不会被画到前面。
//!
//! 判据（`should_freeze`）：下面任意一条成立就放行，否则冻结——
//!
//! - 是我们自己的 `place()` 在改（`SELF_ZORDER`）；
//! - 它带 `WS_EX_TOPMOST`（置顶窗口本来就该在最前）；
//! - 它是前台窗口；
//! - 鼠标正压在它身上（用户马上要点它）；
//! - 它刚刚收到过鼠标按下（`WM_MOUSEACTIVATE`，激活发生在 z 序变化之前，靠这个提前放行）；
//! - 刚做过"抬升全部"（托盘）。
//!
//! 因为是"原地不动"而不是"压到底部"，用户点 A 时 B 停在**它原本的位置**——
//! 如果 B 是用户自己抬起来的，它也留在那个位置，不会被 A 顺带下沉。
//!
//! # 其余几条硬约束
//!
//! - 所有 `SetWindowPos` 都带 `SWP_NOOWNERZORDER`：改一个窗口的 z 序时不牵动它的 owner 组。
//! - `HWND_TOP` / `HWND_BOTTOM` 不会清 `WS_EX_TOPMOST`，只有 `HWND_NOTOPMOST` 会，
//!   所以"取消置顶"必须显式走 `HWND_NOTOPMOST`。
//! - tao 的 `set_always_on_top(false)` 走 `HWND_NOTOPMOST` 且**不带** `SWP_NOOWNERZORDER`，
//!   还带 `SWP_ASYNCWINDOWPOS`（异步，晚到的消息会覆盖事后修正），所以不调它。
//!
//! # 已知限制
//!
//! explorer.exe 重启后 `Progman` 句柄变化，owner 绑定失效（便签退回普通窗口，Win+D 期间
//! 会被桌面盖住；`find_progman` 静默跳过）。重启便签即可恢复。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{Runtime, WebviewWindow};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetPropW, GetWindowLongPtrW, SetPropW, SetWindowLongPtrW, SetWindowPos, WindowFromPoint,
    GWL_EXSTYLE, GWLP_HWNDPARENT, GWLP_WNDPROC, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_STYLECHANGING, WM_WINDOWPOSCHANGING, WNDPROC,
    WS_EX_TOPMOST,
};

/// 原窗口过程存在窗口属性里（不走全局表，销毁时随窗口一起消失）
const OLD_PROC_PROP: windows::core::PCWSTR = w!("sticky_old_wndproc");
/// "最近一次鼠标按下"的时刻（ms），同样挂在窗口属性上
const PRESS_PROP: windows::core::PCWSTR = w!("sticky_last_press");

/// 自己发起的 z 序改动，不要被钩子拦掉
static SELF_ZORDER: AtomicBool = AtomicBool::new(false);

/// 「抬升全部」的豁免截止时刻（ms）：托盘一次抬起所有便签，这期间的 z 序改动都该放行
static QUIET_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// 鼠标按下之后多久内允许自由改 z 序（覆盖"按下 → 激活 → 系统抬组"这段时序）
const PRESS_GRACE_MS: u64 = 800;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 对外入口

/// 创建窗口后调一次（`show()` 之后、`set_focus()` 之前）：绑 owner + 装 z 序钩子。
///
/// 只改 owner，**不动 z 序**：新建窗口本来就在最前面，正是"刚新建 / 刚启动"的预期。
pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(raw) = window.hwnd() else {
        eprintln!("[desktop-bind] 取窗口句柄失败，跳过桌面绑定");
        return;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);

    let Some(progman) = find_progman() else {
        eprintln!("[desktop-bind] 找不到 Progman，跳过桌面绑定");
        return;
    };
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        install_hook(hwnd);
    }
}

/// 窗口销毁前调：把窗口过程还回去（窗口属性会随窗口自动释放，这里只是礼貌）
pub fn release<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(raw) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    let old = unsafe { GetPropW(hwnd, OLD_PROC_PROP) };
    if old.0.is_null() {
        return;
    }
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, old.0 as isize);
    }
}

/// 用户切换图钉：只改这一张的层级，别的窗口一个字节都不动。返回**实际**生效的置顶状态。
///
/// - 置顶 → `HWND_TOPMOST`，进入置顶层最前；
/// - 取消置顶 → `HWND_NOTOPMOST`（唯一能清 `WS_EX_TOPMOST` 的方式），结果是留在普通层最前
///   而**不沉降**——用户要的是"别再压住别的置顶内容"，不是"消失"。
pub fn set_user_pinned<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) -> bool {
    let Ok(raw) = window.hwnd() else {
        return false;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    // 取消置顶后这张可能会被钩子当成"不该动"而冻结，放行一次让 HWND_NOTOPMOST 生效
    place(hwnd, if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST });
    has_topmost(hwnd)
}

/// 把**所有**便签抬到前台应用之上（托盘左键 / `--raise-all`）；返回抬起的窗口数。
///
/// 用户显式要求的全局动作，本来就要一次抬起全部。倒序贴顶让入参第一张最终落在最前。
pub fn raise_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    QUIET_UNTIL_MS.store(now_ms() + 1200, Ordering::Relaxed);

    let mut count = 0;
    for window in windows.iter().rev() {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        place(hwnd, if has_topmost(hwnd) { HWND_TOPMOST } else { HWND_TOP });
        count += 1;
    }
    if let Some(window) = windows.first() {
        let _ = window.set_focus();
    }
    count
}

/// 未置顶的便签全部垫底（`--sink-notes`，回归脚本复位用）；返回沉底的数量。
pub fn sink_all_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    let mut sunk = 0;
    for window in windows {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        if has_topmost(hwnd) {
            continue;
        }
        place(hwnd, HWND_BOTTOM);
        sunk += 1;
    }
    sunk
}

// ---------------------------------------------------------------- 只读查询

/// 窗口此刻是否真的在置顶层：读 `WS_EX_TOPMOST`，不信任何内存状态
pub fn is_always_on_top<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    match window.hwnd() {
        Ok(raw) => has_topmost(HWND(raw.0 as *mut core::ffi::c_void)),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- z 序钩子

/// 把窗口过程换成 `note_wndproc`，原过程存进窗口属性
unsafe fn install_hook(hwnd: HWND) {
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, note_wndproc as *const () as usize as isize) };
    if previous != 0 {
        unsafe {
            let _ = SetPropW(hwnd, OLD_PROC_PROP, Some(HANDLE(previous as *mut _)));
        }
    }
}

/// 窗口过程：唯一职责是在系统要改这张便签的 z 序时判断该不该放行
unsafe extern "system" fn note_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE | WM_LBUTTONDOWN => {
            // 激活发生在系统抬组之前，先记下"用户刚碰过这张"，后续 z 序改动一律放行
            unsafe {
                let _ = SetPropW(
                    hwnd,
                    PRESS_PROP,
                    Some(HANDLE((now_ms() + PRESS_GRACE_MS) as *mut _)),
                );
            }
        }
        WM_STYLECHANGING => {
            // 只有我们自己（place() / set_user_pinned）能翻转 WS_EX_TOPMOST。
            //
            // 起因：tao 的 `WindowFlags::apply_diff()` 收尾时无条件执行
            //   `SetWindowLongW(hwnd, GWL_EXSTYLE, new.to_window_styles().1)`
            // ——`style_ex` 是按 **tao 内部记录的 flags** 重新拼出来的。而我们切换置顶时刻意
            // 绕开 tao 直接调 SetWindowPos（tao 的 set_always_on_top 带 SWP_ASYNCWINDOWPOS
            // 且不带 SWP_NOOWNERZORDER），于是 tao 那边的 ALWAYS_ON_TOP 位永远是创建时的值。
            // 之后任何一次 tao flags 重算（关到托盘再唤回 = set_visible、最小化等）都会拿旧
            // 模型覆盖整条 exstyle，把 WS_EX_TOPMOST 悄悄抹掉，且没有任何路径会把它恢复
            // ——`apply_pinned` 因为"值没变"直接返回，`raise_notes` 看到 has_topmost=false
            // 就走 HWND_TOP。表现就是：数据里 pinned=true，窗口却沉在普通层，
            // 诊断报「置顶一致」「置顶在最前」两条 violation。
            //
            // 实测日志（修复前）：`[trace-topmost] B old=0x40118 new=0x40110`。
            //
            // 这里把 styleNew 的该位锁成 styleOld 的值，等价于"除我们自己以外改不动置顶位"。
            if wparam.0 as i32 == GWL_EXSTYLE.0 && !SELF_ZORDER.load(Ordering::SeqCst) {
                let ss = lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::STYLESTRUCT;
                if !ss.is_null() {
                    unsafe {
                        let keep = (*ss).styleOld & (WS_EX_TOPMOST.0 as u32);
                        (*ss).styleNew = ((*ss).styleNew & !(WS_EX_TOPMOST.0 as u32)) | keep;
                    }
                }
            }
        }
        WM_WINDOWPOSCHANGING => {
            if should_freeze(hwnd) {
                let pos = lparam.0 as *mut windows::Win32::UI::WindowsAndMessaging::WINDOWPOS;
                if !pos.is_null() {
                    unsafe {
                        (*pos).flags |= SWP_NOZORDER;
                    }
                }
            }
        }
        _ => {}
    }

    let old = unsafe { GetPropW(hwnd, OLD_PROC_PROP) };
    if old.0.is_null() {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    let proc: WNDPROC = unsafe { std::mem::transmute(old.0) };
    unsafe { CallWindowProcW(proc, hwnd, msg, wparam, lparam) }
}

/// 这次 z 序改动是否"与这张便签无关"，该原封不动地拦下来
fn should_freeze(hwnd: HWND) -> bool {
    if SELF_ZORDER.load(Ordering::Relaxed) {
        return false; // 我们自己要改
    }
    if now_ms() < QUIET_UNTIL_MS.load(Ordering::Relaxed) {
        return false; // 托盘"显示全部"刚抬过
    }
    if has_topmost(hwnd) {
        return false; // 置顶窗口本来就在最前
    }
    if unsafe { GetForegroundWindow() } == hwnd {
        return false; // 它就是前台
    }
    // 用户刚按过它（激活连带紧跟其后）
    let pressed = unsafe { GetPropW(hwnd, PRESS_PROP) };
    if !pressed.0.is_null() && now_ms() < pressed.0 as usize as u64 {
        return false;
    }
    // 鼠标正压在它身上
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        if unsafe { WindowFromPoint(point) } == hwnd {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------- Win32 杂活

/// 改一个窗口的层级：不动位置尺寸、不抢焦点、不牵动它的 owner 组，且不被自己的钩子拦掉
fn place(hwnd: HWND, after: HWND) {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    SELF_ZORDER.store(true, Ordering::SeqCst);
    unsafe {
        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, flags);
    }
    SELF_ZORDER.store(false, Ordering::SeqCst);
}

fn ex_style(hwnd: HWND) -> isize {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }
}

fn has_topmost(hwnd: HWND) -> bool {
    ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0
}

fn class_is(hwnd: HWND, expected: &str) -> bool {
    let mut buffer = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
    len > 0 && String::from_utf16_lossy(&buffer[..len as usize]) == expected
}

/// 枚举找桌面窗口：`FindWindow("Progman", None)` 在本机返回 0，只能靠 `EnumWindows`
fn find_progman() -> Option<HWND> {
    struct Scan {
        found: isize,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let scan = &mut *(lparam.0 as *mut Scan);
        if class_is(hwnd, "Progman") {
            scan.found = hwnd.0 as isize;
            return false.into();
        }
        true.into()
    }

    let mut scan = Scan { found: 0 };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut scan as *mut Scan as isize));
    }
    if scan.found == 0 {
        None
    } else {
        Some(HWND(scan.found as *mut core::ffi::c_void))
    }
}
