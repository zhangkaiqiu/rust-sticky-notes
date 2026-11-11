//! 便签的桌面层级：抗 Win+D + 图钉 + 「操作 A 不得改变 B」。
//!
//! # 为什么 owner = Progman
//!
//! `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 把桌面窗口设成便签的 owner。
//! owner 窗口永远在它的 owner 之上，于是 Win+D 时 shell 把 `Progman` 抬起来、便签跟着
//! 浮在桌面上——不需要任何置顶切换，也就没有置顶/取消置顶带来的重绘跳动。
//! 除此之外便签就是普通窗口：前台应用能正常盖住它，被点中也能正常到最前。
//!
//! # 唯一需要擦的屁股：owner 组的激活连带
//!
//! owner 相同的窗口在 Windows 眼里是一个 z-order 组。**激活组内任意一张**，系统会把整个
//! 组抬到普通层最前面——包括那些本该沉在底下的兄弟。实测：A 置顶、B 沉底（z=25）、
//! 记事本在前台（z=4），仅仅激活 A，B 就被系统搬到 z=4，窜到记事本之上了。这**不是**
//! 本模块的代码干的（那一刻我们一行 `SetWindowPos` 都没调），是 Windows 的组语义。
//!
//! 所以这里只做一件事：**激活某张便签之后，把「未置顶且不是当前激活窗口」的其它便签
//! 重新垫到底部**（`settle_siblings`）。它们本来就该待在那里，这一步不会改变任何窗口的
//! exstyle、不影响置顶状态、也不动被激活的那张。
//!
//! 于是"操作 A 改变 B"的路径被压缩成一个必然可覆盖的动作：B 任何时候都不是"被 A 抬起"，
//! 而是"被系统抬起后立刻按它自己的常态放回去"。
//!
//! # 历史坑位速查（结论；过程见 README「层级与置顶」）
//!
//! - 所有 `SetWindowPos` 都必须带 `SWP_NOOWNERZORDER`。不带时改一个窗口的 z 序会顺手
//!   清掉兄弟窗口的 `WS_EX_TOPMOST`、或把兄弟一起抬到普通层最前。
//! - 0.4.2 试用过"声明式全量 reconcile"（每张便签记 `pinned` / `raised` 两个状态位，
//!   每次改动后给所有窗口重算位置）。它的问题不是算错，而是**每次激活都重写全部窗口的
//!   z 序**：只要某张兄弟残留了 `raised=true`（取消置顶时会写进去），点 A 一下就会把它
//!   顶到浏览器前面。状态位还会泄漏——没有任何路径把它清回 false。0.4.3 整段删除。
//! - tao 的 `set_always_on_top(false)` 走 `HWND_NOTOPMOST` 且**不带** `SWP_NOOWNERZORDER`，
//!   会牵动整组兄弟，所以这里不调它，自己带齐标志做同一步（`place`）。
//! - `HWND_TOP` / `HWND_BOTTOM` 不会清 `WS_EX_TOPMOST`，只有 `HWND_NOTOPMOST` 会。
//!
//! # 已知限制
//!
//! explorer.exe 重启后 `Progman` 句柄变化，owner 绑定失效（便签退回普通窗口，Win+D 期间
//! 会被桌面盖住；此时 `find_progman` 静默跳过）。重启便签即可恢复。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongPtrW,
    SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWLP_HWNDPARENT, GW_OWNER, HWND_BOTTOM,
    HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOOWNERZORDER, SWP_NOSIZE, WS_EX_TOPMOST,
};

/// tao 在 Windows 上给每个窗口用的类名，也是筛选"本进程便签窗口"的依据
const NOTE_CLASS: &str = "Tauri Window";

/// 激活之后补垫兄弟窗口的持续时间（毫秒）。
///
/// 激活 owner 组会把整组抬到普通层最前，这一步由系统完成、无法阻止，只能事后放回去。
/// 而且它发生在**我们的 `Focused(true)` 回调之后**：单次补垫、甚至 90ms 后的一次补垫，
/// 都会被系统随后到达的连带覆盖（实测 4 次补垫仍偶发"第一次采样还是抬起的"）。
/// 所以这里在 `SETTLE_WINDOW_MS` 内持续补，谁后到就由谁收敛。
const SETTLE_WINDOW_MS: u64 = 150;

/// 补垫的间隔：够密才能压住系统的连带，又不至于空转耗 CPU。
const SETTLE_STEP_MS: u64 = 10;

/// 「显式抬升全部便签」的豁免截止时刻（ms，`0` 表示没有豁免）。
///
/// 托盘抬升会把**每一张**便签都放到前台应用之上，紧接着的 `set_focus` 会触发一次
/// `Focused(true)`——如果不豁免，补垫逻辑会立刻把除焦点窗口外的其它便签踩回底部，
/// 用户看到的是"点托盘只有一张上来"。所以抬升之后给一个静默期。
static QUIET_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// 是否已经有一个"延迟补垫"在排队（避免用户连点几十下就起几十个线程）
static SETTLE_PENDING: AtomicBool = AtomicBool::new(false);

/// 当前毫秒时间戳（只做短期比较，用系统时钟足够）
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 对外入口

/// 创建窗口后调一次（`show()` 之后、`set_focus()` 之前）：只把句柄的 owner 绑到桌面，不动 z 序。
///
/// 新建窗口本来就在最前面，这正是"刚新建 / 刚启动"的预期；再垫一次底只会让便签躲到
/// 别的应用后面。之后用户切到别的应用，前台应用自然盖住它。
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
    }
}

/// 用户切换图钉：只改这一张的层级，兄弟窗口一个字节都不动。返回**实际**生效的置顶状态。
///
/// - 置顶 → `HWND_TOPMOST`，进入置顶层最前；
/// - 取消置顶 → `HWND_NOTOPMOST`（唯一能清 `WS_EX_TOPMOST` 的方式），结果是留在普通层最前
///   而**不沉降**——用户要的是"别再压住别的置顶内容"，不是"消失"。
///
/// 这里**不调** tao 的 `set_always_on_top`：它不带 `SWP_NOOWNERZORDER`，会顺手把整组兄弟
/// 拽起来。
pub fn set_user_pinned<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) -> bool {
    let Ok(raw) = window.hwnd() else {
        return false;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    place(hwnd, if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST });
    has_topmost(hwnd)
}

/// 某张便签被激活**之后**调：在 `SETTLE_WINDOW_MS` 内反复把其它「未置顶、且不是前景窗口」的
/// 便签垫回底部。
///
/// 为什么要反复：`Focused(true)` 到达时系统完成激活连带还有一段时序，单次重排会被随后到达的
/// 连带覆盖（实测当场无效，90ms 后也偶发被覆盖）。持续补一小段，保证最后一个动作是我们做的。
/// `SETTLE_PENDING` 把连续点击合并成一次。
///
/// 只做这一件事，因为激活 owner 组会把整组抬起来（见模块注释）。被激活的那张不动——
/// 用户点它就是要它在前面。
pub fn settle_siblings<R: Runtime>(_activated: &WebviewWindow<R>) {
    if now_ms() < QUIET_UNTIL_MS.load(Ordering::Relaxed) {
        return; // 刚做过"抬升全部"，别把兄弟踩回去
    }
    if SETTLE_PENDING.swap(true, Ordering::AcqRel) {
        return; // 已经排了一个，等它跑完再说
    }
    std::thread::spawn(|| {
        let start = now_ms();
        while now_ms() - start < SETTLE_WINDOW_MS {
            settle_now();
            std::thread::sleep(std::time::Duration::from_millis(SETTLE_STEP_MS));
        }
        SETTLE_PENDING.store(false, Ordering::Release);
    });
}

/// 真正补垫：把「未置顶、且不是前景窗口」的便签放到普通层最底。
fn settle_now() {
    if now_ms() < QUIET_UNTIL_MS.load(Ordering::Relaxed) {
        return;
    }
    let foreground = unsafe { GetForegroundWindow() };
    let Some(progman) = find_progman() else {
        return;
    };

    for hwnd in note_windows(progman) {
        if hwnd == foreground || has_topmost(hwnd) {
            continue;
        }
        place(hwnd, HWND_BOTTOM);
    }
}

/// 把**所有**便签抬到前台应用之上（托盘左键 / `--raise-all`）；返回抬起的窗口数。
///
/// 用户显式要求的全局动作，本来就要一次抬起全部。倒序贴顶让入参第一张最终落在最前，
/// 随后聚焦它；同时开一段静默期，免得这次 `Focused(true)` 把其余便签又踩回去。
pub fn raise_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    QUIET_UNTIL_MS.store(now_ms() + 1200, Ordering::Relaxed);

    let mut count = 0;
    for window in windows.iter().rev() {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        // 置顶的已经在置顶层，用 HWND_TOPMOST 重贴一次即可；普通窗口贴到普通层最前
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

// ---------------------------------------------------------------- Win32 杂活

/// 改一个窗口的层级：不动位置尺寸、不抢焦点、不牵动同 owner 组的兄弟窗口
fn place(hwnd: HWND, after: HWND) {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    unsafe {
        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, flags);
    }
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

/// 本进程的所有便签窗口（类名匹配 + owner 仍是 Progman，即桌面绑定还在）
fn note_windows(progman: HWND) -> Vec<HWND> {
    struct Scan {
        progman: HWND,
        found: Vec<HWND>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let scan = &mut *(lparam.0 as *mut Scan);
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
        if owner == scan.progman && class_is(hwnd, NOTE_CLASS) {
            scan.found.push(hwnd);
        }
        true.into()
    }

    let mut scan = Scan {
        progman,
        found: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut scan as *mut Scan as isize));
    }
    scan.found
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
