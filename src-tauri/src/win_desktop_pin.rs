//! 便签的桌面层级：抗 Win+D、图钉、以及"操作 A 不改变 B"的声明式重排。
//!
//! ## 模型：状态 + 一次全量重排
//!
//! 每个便签窗口只记两个语义状态位：
//!
//! | 字段 | 含义 |
//! | --- | --- |
//! | `pinned` | 用户钉住了吗（⟺ 窗口带 `WS_EX_TOPMOST`） |
//! | `raised` | 是否被显式抬到前面（托盘 / 被点中 / 刚取消置顶） |
//!
//! 任何入口都只做两件事：**改状态 → 调 `reconcile()`**。没有"每条路径补一刀"的碎补丁，
//! 于是"操作 A 顺带改变了 B"变成一个单点可覆盖的问题：B 在重排里必然被放回它该在的位置。
//!
//! ## 期望顺序（同一个 owner 组内，从前往后）
//!
//! 1. `pinned = true`：置顶层，`HWND_TOPMOST`；
//! 2. `raised = true` 且未置顶：普通层最前，`HWND_TOP`；
//! 3. 其余（沉底）：普通层最底，`HWND_BOTTOM`。
//!
//! 段内前后按注册顺序（= store 顺序）。重排从最后一段往最前一段贴：
//! 沉底段正序（`SetWindowPos` 贴底时先贴的最终更靠前），抬升段与置顶段倒序（贴顶时后贴的
//! 最终更靠前）。全部带 `SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER`。
//!
//! ## 为什么 owner = Progman
//!
//! `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 把桌面窗口设成便签的 owner。owner
//! 窗口永远在它的 owner 之上，于是：平时便签垫在所有普通窗口之下（前台应用正常覆盖它），
//! Win+D 时 shell 把 `Progman` 抬起来、便签跟着浮在桌面上——不需要任何置顶切换，也就没有
//! 置顶/取消置顶带来的重绘跳动。新建窗口默认在最前面，所以启动时要垫一次底（见 `install`）。
//!
//! ## 历史坑位速查（结论；过程见 README「层级与置顶」）
//!
//! - **所有** `SetWindowPos` 都必须带 `SWP_NOOWNERZORDER`。owner 同为 Progman 的窗口是
//!   一个 z-order 组，不带这个标志时改一个窗口的 z 序会顺手清掉兄弟窗口的
//!   `WS_EX_TOPMOST`、或把兄弟一起抬到普通层最前。
//! - **激活**（点击）组内窗口会让 Windows 把整组兄弟抬到普通层最前，这条不受
//!   `SWP_NOOWNERZORDER` 约束，只能事后重排（`Focused(true)` → `note_activated`）。
//! - tao 的 `set_always_on_top(false)` 走 `HWND_NOTOPMOST` 且**不带** `SWP_NOOWNERZORDER`：
//!   既把窗口顶到普通层最前，又牵动整组。所以这里不调它，自己做同一步（`place`）。
//! - `HWND_TOP` / `HWND_BOTTOM` 不会清掉 `WS_EX_TOPMOST`，只有 `HWND_NOTOPMOST` 会。
//!
//! ## 已知限制
//!
//! explorer.exe 重启后 `Progman` 句柄变化，owner 绑定失效（便签退回普通窗口，Win+D 期间
//! 会被桌面盖住；此时 `find_progman` 与 `reconcile` 都会静默跳过）。重启便签即可恢复。

use std::sync::{Mutex, MutexGuard};

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, IsWindow, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, GWLP_HWNDPARENT, GW_OWNER, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    WS_EX_TOPMOST,
};

/// tao 在 Windows 上给每个窗口用的类名，也是筛选"本进程便签窗口"的依据
const NOTE_CLASS: &str = "Tauri Window";

// ---------------------------------------------------------------- 语义状态

/// 一个便签窗口的语义状态
#[derive(Clone, Copy, Default, Debug)]
struct NoteLayer {
    /// 用户钉住了吗；`reconcile` 保证它 ⟺ 窗口带 `WS_EX_TOPMOST`
    pinned: bool,
    /// 是否被显式抬到前面（置顶的窗口这个位无意义，它总在第一段）
    raised: bool,
}

/// 状态表：按**注册顺序**排列（= 建窗口的顺序，即 store 顺序），段内前后就靠它。
/// 便签是两位数以内，线性查找比哈希表更省事，也不会丢掉顺序。
static LAYERS: Mutex<Vec<(isize, NoteLayer)>> = Mutex::new(Vec::new());

/// 取锁（中毒也继续用：只有我们自己写，坏不掉）
fn lock() -> MutexGuard<'static, Vec<(isize, NoteLayer)>> {
    match LAYERS.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn index_of(layers: &[(isize, NoteLayer)], key: isize) -> Option<usize> {
    layers.iter().position(|(hwnd, _)| *hwnd == key)
}

/// 写一个窗口的语义状态；第一次见到的 hwnd 顺带登记到末尾
fn set_state(hwnd: HWND, update: impl FnOnce(&mut NoteLayer)) {
    let key = hwnd_key(hwnd);
    let mut layers = lock();
    match index_of(&layers, key) {
        Some(index) => update(&mut layers[index].1),
        None => {
            let mut state = NoteLayer::default();
            update(&mut state);
            layers.push((key, state));
        }
    }
}

/// 读一个窗口的语义状态；没登记过就是沉底（开机默认）
fn state_of(hwnd: HWND) -> NoteLayer {
    let layers = lock();
    match index_of(&layers, hwnd_key(hwnd)) {
        Some(index) => layers[index].1,
        None => NoteLayer::default(),
    }
}

// ---------------------------------------------------------------- 重排

/// 一个窗口的目标：要放到哪儿、段内排第几
struct Target {
    hwnd: HWND,
    layer: NoteLayer,
    /// 段内顺序（注册顺序 = store 顺序）；未登记的排最后
    order: usize,
}

/// 按"从前往后"排好的三段：置顶层 / 抬升段 / 沉底段
fn plan(targets: &[Target]) -> (Vec<HWND>, Vec<HWND>, Vec<HWND>) {
    let mut sorted: Vec<&Target> = targets.iter().collect();
    sorted.sort_by_key(|target| target.order);

    let (mut pinned, mut raised, mut sunk) = (Vec::new(), Vec::new(), Vec::new());
    for target in sorted {
        if target.layer.pinned {
            pinned.push(target.hwnd);
        } else if target.layer.raised {
            raised.push(target.hwnd);
        } else {
            sunk.push(target.hwnd);
        }
    }
    (pinned, raised, sunk)
}

/// 把 `targets` 一次性摆到期望层级
fn apply(targets: &[Target]) {
    // 1) 先把 `WS_EX_TOPMOST` 与 `pinned` 摆一致。取消置顶必须走 `HWND_NOTOPMOST`
    //    （`HWND_TOP` / `HWND_BOTTOM` 不会清这个位），它同时会把窗口顶到普通层最前，
    //    紧接着第 2 步会把它放回该去的位置，所以这个副作用不用单独处理。
    for target in targets {
        let want = target.layer.pinned;
        if has_topmost(target.hwnd) != want {
            place(target.hwnd, if want { HWND_TOPMOST } else { HWND_NOTOPMOST });
        }
    }

    // 2) 从后段往前面段贴，段内前后 = 注册顺序。
    let (pinned, raised, sunk) = plan(targets);
    for &hwnd in &sunk {
        place(hwnd, HWND_BOTTOM);
    }
    for &hwnd in raised.iter().rev() {
        place(hwnd, HWND_TOP);
    }
    for &hwnd in pinned.iter().rev() {
        place(hwnd, HWND_TOPMOST);
    }
}

/// 全量重排：调用方只负责改状态，位置统一在这里算。
///
/// 最多两轮、不轮询：正常情况下第一轮就收敛；只有回读发现 `WS_EX_TOPMOST` 仍与 `pinned`
/// 不一致（极端时序下才会有）才补第二轮。
fn reconcile() {
    let Some(progman) = find_progman() else {
        return; // explorer 重启过：owner 绑定失效，这时候动 z 序只会添乱
    };
    let targets = collect_targets(progman);
    if targets.is_empty() {
        return;
    }

    for round in 0..2 {
        apply(&targets);
        if targets.iter().all(|t| has_topmost(t.hwnd) == t.layer.pinned) {
            break;
        }
        if round == 1 {
            eprintln!("[zorder] 重排两轮后 TOPMOST 位仍与 pinned 不一致");
        }
    }
    debug_log(&targets, "reconcile");
}

/// 采集本进程所有便签窗口的当前状态，按注册顺序返回。
///
/// 顺手清掉**真正销毁**的窗口的登记（用 `IsWindow` 判断）：被隐藏 / 最小化的便签句柄
/// 依然有效，它的"沉底"意图要留着，否则再显示出来就会飘到最前面。
fn collect_targets(progman: HWND) -> Vec<Target> {
    let live = note_windows(progman);
    let mut layers = lock();
    layers.retain(|(key, _)| unsafe { IsWindow(Some(HWND(*key as *mut core::ffi::c_void))).as_bool() });

    live.iter()
        .map(|&hwnd| {
            let index = index_of(&layers, hwnd_key(hwnd));
            Target {
                hwnd,
                layer: index.map(|i| layers[i].1).unwrap_or_default(),
                order: index.unwrap_or(usize::MAX),
            }
        })
        .collect()
}

/// 改一个窗口的层级：不动位置尺寸、不抢焦点、不牵动同 owner 组的兄弟窗口
fn place(hwnd: HWND, after: HWND) {
    let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
    unsafe {
        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, flags);
    }
}

// ---------------------------------------------------------------- 对外入口
// ---------------------------------------------------------------- （全部是"改状态 → reconcile"）

/// 创建窗口后调一次（要在 `show()` 之后、`set_focus()` 之前）：登记状态、绑定 owner、重排。
///
/// 这里写入的是**开机默认态**：未置顶的便签从沉底开始（否则新建窗口默认在最前面）。
/// 紧接着建窗 / `show()` / `set_focus()` 会触发一串 `Focused(true)`，把它们记成
/// `raised`——**这是预期的**：用户刚启动应用，便签出现在最前面很正常。用户切去别的
/// 应用后，前台应用照样能盖住它（靠的是 owner = Progman 的层级关系，不是 TOPMOST）。
pub fn install<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) {
    let Ok(raw) = window.hwnd() else {
        eprintln!("[desktop-bind] 取窗口句柄失败，跳过桌面绑定");
        return;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    set_state(hwnd, |state| {
        state.pinned = pinned;
        state.raised = pinned;
    });

    let Some(progman) = find_progman() else {
        eprintln!("[desktop-bind] 找不到 Progman，跳过桌面绑定");
        return;
    };
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
    }
    reconcile();
}

/// 用户切换图钉：只改状态位，然后重排。返回**实际**生效的置顶状态（回读 exstyle）。
///
/// 置顶与取消置顶都标记 `raised`：置顶本来就在最前，取消置顶则按"留在原处、不沉降"
/// 处理（用户要的是"别再压住别的置顶内容"，不是"消失"）。
///
/// 注意这里**不调** tao 的 `set_always_on_top`：它走 `HWND_NOTOPMOST` 且不带
/// `SWP_NOOWNERZORDER`，会顺手把整组兄弟拽起来（换层级这一步由 `reconcile` 做）。
pub fn set_user_pinned<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) -> bool {
    let Ok(raw) = window.hwnd() else {
        return false;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    set_state(hwnd, |state| {
        state.pinned = pinned;
        state.raised = true;
    });
    reconcile();
    has_topmost(hwnd)
}

/// 便签被激活之后调用（`WindowEvent::Focused(true)` / 程序 focus）。
///
/// 激活一个 owner = Progman 的窗口时，Windows 会把整组兄弟抬到普通层最前。这里把被激活的
/// 那张记成 `raised` 再重排：它留在前面，后台仍沉底的兄弟被压回去（第 6 条行为）。
pub fn note_activated<R: Runtime>(activated: &WebviewWindow<R>) {
    let Ok(raw) = activated.hwnd() else {
        return;
    };
    set_state(HWND(raw.0 as *mut core::ffi::c_void), |state| {
        state.raised = true;
    });
    reconcile();
}

/// 把**所有**便签抬到前台应用之上（托盘左键 / `--raise-all`）；返回抬起的窗口数。
///
/// 全部标记 `raised` 后重排，最后只对 store 里的第一张 `set_focus()` 一次：随后的
/// `Focused(true)` 因为兄弟都已是 `raised` 而不会再被踩回去。返回的数量只用于诊断输出。
pub fn raise_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    let mut count = 0;
    for window in windows {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        set_state(HWND(raw.0 as *mut core::ffi::c_void), |state| {
            state.raised = true;
        });
        count += 1;
    }
    reconcile();
    if let Some(window) = windows.first() {
        let _ = window.set_focus();
    }
    count
}

/// 复位成"开机默认"：未置顶的便签全部沉底（`--sink-notes`）；返回沉底的数量。
pub fn sink_all_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    let mut sunk = 0;
    for window in windows {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        if state_of(hwnd).pinned {
            continue;
        }
        set_state(hwnd, |state| state.raised = false);
        sunk += 1;
    }
    reconcile();
    sunk
}

// ---------------------------------------------------------------- 只读查询（诊断用）

/// 窗口此刻是否真的在置顶层：读 `WS_EX_TOPMOST`，不信任何内存状态
pub fn is_always_on_top<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    match window.hwnd() {
        Ok(raw) => has_topmost(HWND(raw.0 as *mut core::ffi::c_void)),
        Err(_) => false,
    }
}

/// 这张便签是否被标记为"已被显式抬到前面"（`--dump` 的 `raised` 字段）
pub fn is_raised(hwnd: isize) -> bool {
    state_of(HWND(hwnd as *mut core::ffi::c_void)).raised
}

// ---------------------------------------------------------------- Win32 杂活

fn hwnd_key(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

fn ex_style(hwnd: HWND) -> isize {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }
}

fn has_topmost(hwnd: HWND) -> bool {
    ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0
}

/// 排查开关：只有 `STICKY_DEBUG_ZORDER=1` 时才打印层级日志
fn debug_enabled() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        std::env::var("STICKY_DEBUG_ZORDER")
            .map(|value| value == "1")
            .unwrap_or(false)
    })
}

fn debug_log(targets: &[Target], tag: &str) {
    if !debug_enabled() {
        return;
    }
    for target in targets {
        eprintln!(
            "[zorder] {tag} hwnd={} pinned={} raised={} topmost={} ex=0x{:X}",
            hwnd_key(target.hwnd),
            target.layer.pinned,
            target.layer.raised,
            has_topmost(target.hwnd),
            ex_style(target.hwnd)
        );
    }
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
