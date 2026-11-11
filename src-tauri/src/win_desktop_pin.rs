//! 抵抗 Win+D（以及"显示桌面"）：把便签"绑定到桌面"。
//!
//! ## 做法
//! `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 把桌面窗口设成便签的 owner。
//! owner 窗口永远显示在它的 owner 之上，于是：
//! - 平时：便签垫在所有普通窗口之下，前台应用正常覆盖它（符合要求）；
//! - Win+D：shell 把应用最小化、把 `Progman` 抬起来，便签因为"在 owner 之上"自然
//!   浮在桌面上，不需要任何置顶操作，也就没有置顶切换带来的重绘跳动；
//! - 再按 Win+D：应用恢复，便签重新被盖住。
//!
//! 换句话说，便签成了桌面的一部分：桌面在，它就在。
//!
//! ## 为什么要额外 `HWND_BOTTOM`
//! 新建窗口默认在最前面；`HWND_NOTOPMOST`（取消图钉时 tao 会调用）会把窗口放到
//! **普通层最上面**，于是取消置顶后便签反而挡住其它应用。实测 `HWND_BOTTOM` 在
//! owner 绑定下等价于"贴着自己的 owner"——压在壁纸之上、其它窗口之下，正是想要的层级。
//! 所以**启动时**要垫一次（`install()`）。
//!
//! 0.4.1 起**取消置顶不再垫底**（见下面「0.4.1 修正」）：用户点掉图钉时窗口应当停在
//! 原处，只是不再压住别的置顶内容；直接 `HWND_BOTTOM` 在用户眼里等于"便签消失了"。
//!
//! ## 0.3.1 修正：owner 组会被 z-order 操作"顺带"改动（两个用户 bug 的根因）
//!
//! 同一个 owner 名下的多个窗口在 Windows 眼里属于**同一个 z-order 组**。实测（见下）
//! 发现两个致命副作用，都出在 `SetWindowPos` 漏了 `SWP_NOOWNERZORDER`：
//!
//! 1. **BUG 1「点图钉把后面那张也拉起来」**：用户点图钉后窗口被激活；激活一个
//!    owner = Progman 的窗口时，Windows 会把它的**兄弟窗口（同一 owner 组的其它便签）**
//!    一起抬到普通层最前面。实测：激活 A 前后，B 的 z 序从 28 直接变成 7，
//!    越过了前台浏览器（z=10）。所以问题不在"点图钉"本身，而在"被激活"这一步。
//!    修法：所有层级操作显式带 `SWP_NOOWNERZORDER` + `SWP_NOACTIVATE`，并且激活后
//!    把所有**未置顶**的便签重新 pad 回底部（`park_siblings_behind`）。
//!
//! 2. **BUG 2「重启后图钉显示钉住、实际没置顶」**：pinned 的便签先创建（带
//!    `WS_EX_TOPMOST`），紧接着 unpinned 的那张创建并 `park_behind`；这一步
//!    `SetWindowPos(B, HWND_BOTTOM)` **把兄弟窗口 A 的 `WS_EX_TOPMOST` 位一并清掉了**
//!    （实测 A 从 0x40118 变成 0x40110）。先后顺序决定成败，所以"两张都是 pinned=false"
//!    的旧数据看不出来，一旦有 pinned 就复现。
//!    修法：`park_behind` 加 `SWP_NOOWNERZORDER`；并且在应用完 pinned 之后回读
//!    exstyle 校验，不一致就重试一次（见 `apply_pinned_verified`）。
//!
//! ## 0.4.0 修正：BUG 3「取消一张的置顶，另一张跟着掉出置顶层」
//!
//! `--check` 上线后第一次跑回归就抓到的第三个坑：两张便签都置顶（都是 0x40118）时，
//! 把其中一张取消置顶，**另一张的 `WS_EX_TOPMOST` 也会被清掉**（实测 0x40118 -> 0x40110），
//! 于是数据里 pinned=true、窗口却掉回普通层——正是第 1 条不变量要抓的那种状态。
//!
//! 根因不在我们自己的代码，而在 tao：`set_always_on_top(false)` 走
//! `SetWindowPos(hwnd, HWND_NOTOPMOST, …, SWP_ASYNCWINDOWPOS|SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`
//! （见 tao 0.37 的 `window_state.rs`），**没有 `SWP_NOOWNERZORDER`**，
//! 于是和 BUG 2 同一个机制（owner 组连带）把兄弟窗口的 TOPMOST 位带走了。
//! tao 的调用我们改不了，所以改成"事后修复"：取消置顶的动作做完之后，
//! 把所有**本该置顶**却丢了 TOPMOST 的兄弟窗口重新钉回去（`repair_pinned_topmost`，
//! 不激活、不改尺寸位置、只补标志位）。
//!
//! ## 0.4.1 修正：隐式"沉降"与托盘抬升失效（层位意图 Sunk/Raised）
//!
//! 0.4.0 的收尾动作是"取消置顶就 `park_behind`"和"激活后把所有未置顶便签垫回去"，
//! 结果引出两个用户可见的问题：
//!
//! 1. **BUG A「取消置顶会沉降」**：用户点掉图钉，窗口直接被 `HWND_BOTTOM` 踩到所有窗口
//!    之下，看起来像"消失了"。用户想要的只是"别再压住别的置顶内容"，还待在原处。
//!    修法：取消置顶**不再** `park_behind`；tao 的 `HWND_NOTOPMOST` 自带"落到普通层最上面"
//!    的效果，正好是要的位置，并把它标记为 `Raised`。
//!
//! 2. **BUG B「点托盘只抬一张、另一张反而沉降」**：托盘左键 → `show_all_notes` 只对
//!    最后一个窗口 `set_focus()`；该窗口的 `Focused(true)` 触发 `park_siblings_behind`，
//!    把其它未置顶便签全部 `HWND_BOTTOM`。于是"聚焦那张到前面、另一张被踩到底"，
//!    中间那一瞬的抬起-沉降就是用户看到的"闪一下"。
//!    修法：托盘左键改成显式调用 `raise_notes`，**全部**标记 `Raised` 后逐张
//!    `SetWindowPos(HWND_TOP/HWND_TOPMOST)` 抬到前台应用之上，最后只聚焦一次；
//!    后续 `Focused(true)` 因为兄弟窗口已经是 `Raised` 而不会再被压回去。
//!
//! 这两条修正都落在同一个概念上：**层位意图**（见 `LayerIntent`）。
//! - `Sunk`：这张便签应该待在普通层最下面（开机、以及用户主动 `--sink-notes` 之后）；
//! - `Raised`：这张便签已被用户显式抬到前面，自动逻辑不许再把它踩回去。
//!
//! 规则（改这里之前先想清楚会不会破坏其中任何一条）：
//! - `install()`：`pinned=false` → `Sunk` + `park_behind`；`pinned=true` → `Raised`；
//! - `set_user_pinned(.., false)`：不 park，标记 `Raised`；
//! - `set_user_pinned(.., true)`：标记 `Raised`；
//! - 便签被激活（`Focused(true)`，走 `park_siblings_after_activation`）→ 该窗口标记 `Raised`；
//! - `park_siblings_behind(keep)` **只**处理 `pinned == false && 意图 == Sunk` 的兄弟窗口，
//!   其它一律不动——"用户刚抬起来的便签"不会再被踩下去，
//!   "后台还沉在底部的便签"仍会被压回去（BUG 1 的修复不能丢）；
//! - `repair_pinned_topmost` 保持原样：只补置顶窗口的 TOPMOST 位。
//!
//! 这里**故意不引入**"全局抑制标志"：抑制标志要维护生命周期、容易漏复位；
//! 而 `Raised` 意图是每个窗口自己的状态，`Focused` 事件天然能看到它。
//!
//! ## 多窗口
//! 每张便签是独立窗口，`install` 会对每个窗口各调一次；图钉状态按 HWND 分别记录，
//! 否则一张便签置顶会把所有便签都当成"用户已置顶"，新建窗口就不再垫底。
//!
//! ## 为什么不用上一版的"临时置顶"
//! 那一版靠 `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 判断显示桌面，再
//! `SetWindowPos(HWND_TOPMOST)` 抬起来、退出时撤销。问题是：
//! 1. Win+D 触发瞬间的置顶会被 shell 过渡状态吞掉（返回成功但 `WS_EX_TOPMOST` 不变），
//!    不得不加重试轮询；
//! 2. 置顶/取消置顶各引发一次层级切换，视觉上便签会"跳一下"。
//! owner 绑定把这些全都省掉了。
//!
//! ## 已知限制
//! explorer.exe 重启后 `Progman` 句柄变化，绑定会失效（便签退回普通窗口，Win+D
//! 期间会被桌面盖住）。此时重启便签即可恢复。
//!
//! 另外：`SWP_NOOWNERZORDER` 只能阻止"改 z 序"这一个动作顺手改兄弟窗口，无法阻止
//! Windows 在**激活**窗口时对 owner 组的成组抬起——所以窗口激活后仍需要
//! `park_siblings_behind` 补一刀（只对未置顶的窗口，且带 `SWP_NOACTIVATE`）。

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowTextW, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, GWLP_HWNDPARENT, GW_OWNER, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, WS_EX_TOPMOST,
};

/// `WS_EX_NOACTIVATE`（windows crate 里叫 NOACTIVATE 的 exstyle 位）
const WS_EX_NOACTIVATE: isize = 0x0800_0000;

/// 层位意图：这张便签"应该"待在普通层的哪儿。
///
/// 只描述**用户/程序显式表达的意愿**，不描述窗口此刻真实的 z 序（那要读 `EnumWindows`）。
/// 自动沉降逻辑（`park_siblings_behind`）只对 `Sunk` 的窗口生效，这是 0.4.1 修复
/// 「取消置顶沉降」和「托盘抬升被踩回去」的关键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerIntent {
    /// 应该待在普通层最下面（开机默认；`--sink-notes` 可以手动复位到这里）
    Sunk,
    /// 已被用户显式抬到前面，自动逻辑不许再把它踩回去
    Raised,
}

/// 每个便签窗口（HWND）各自的图钉状态
static USER_PINNED: Mutex<Option<HashMap<isize, bool>>> = Mutex::new(None);

/// 每个便签窗口（HWND）各自的层位意图；缺省（没记录）= `Sunk`
static LAYER_INTENT: Mutex<Option<HashMap<isize, LayerIntent>>> = Mutex::new(None);

fn set_pinned_state(hwnd: HWND, pinned: bool) {
    let key = hwnd.0 as isize;
    let mut guard = match USER_PINNED.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard
        .get_or_insert_with(HashMap::new)
        .insert(key, pinned);
}

/// 写入某个窗口的层位意图
fn set_layer_intent(hwnd: HWND, intent: LayerIntent) {
    let key = hwnd.0 as isize;
    let mut guard = match LAYER_INTENT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.get_or_insert_with(HashMap::new).insert(key, intent);
}

/// 读某个窗口的层位意图；没记录过就是 `Sunk`（开机默认）
fn layer_intent(hwnd: HWND) -> LayerIntent {
    let guard = match LAYER_INTENT.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard
        .as_ref()
        .and_then(|map| map.get(&(hwnd.0 as isize)).copied())
        .unwrap_or(LayerIntent::Sunk)
}

/// 标记为"已被显式抬到前面"（`Sunk -> Raised` 的单向迁移，重复调用无副作用）
fn mark_raised(hwnd: HWND) {
    set_layer_intent(hwnd, LayerIntent::Raised);
    if debug_enabled() {
        eprintln!("[zorder]   mark raised hwnd={}", hwnd.0 as isize);
    }
}

/// 标记回"应该待在底部"
fn mark_sunk(hwnd: HWND) {
    set_layer_intent(hwnd, LayerIntent::Sunk);
    if debug_enabled() {
        eprintln!("[zorder]   mark sunk hwnd={}", hwnd.0 as isize);
    }
}

/// 给 `diagnostics` 用：这张便签窗口是否被标记为"已被显式抬到前面"。
///
/// 返回 `bool` 而不是枚举，是因为 `--dump` 只有 `raised: true/false` 一个字段；
/// `Sunk` 就是这个布尔值的 `false`。参数是裸 HWND（诊断层从 `WebviewWindow` 拿到的
/// 就是这个），调用方不需要再包装一次。
pub fn is_raised(hwnd: isize) -> bool {
    layer_intent(HWND(hwnd as *mut core::ffi::c_void)) == LayerIntent::Raised
}

fn is_user_pinned(hwnd: HWND) -> bool {
    let guard = match USER_PINNED.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard
        .as_ref()
        .and_then(|map| map.get(&(hwnd.0 as isize)).copied())
        .unwrap_or(false)
}

/// 诊断开关：只有 `STICKY_DEBUG_ZORDER=1` 时才打印层级日志
fn debug_enabled() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        std::env::var("STICKY_DEBUG_ZORDER")
            .map(|v| v == "1")
            .unwrap_or(false)
    })
}

/// 临时诊断：打印所有便签窗口的 exstyle / owner / z 序索引
fn debug_dump(tag: &str) {
    if !debug_enabled() {
        return;
    }
    let notes = collect_note_windows();
    let zorder = collect_zorder();
    let fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };

    eprintln!("[zorder] ---- {tag} ----");
    for (hwnd, title) in &notes {
        let ex = ex_style(*hwnd);
        let owner = unsafe { GetWindow(*hwnd, GW_OWNER) }.unwrap_or_default();
        let z = zorder
            .iter()
            .position(|h| *h == *hwnd)
            .map(|i| i.to_string())
            .unwrap_or_else(|| "?".to_string());
        eprintln!(
            "[zorder]   {title} hwnd={} ex=0x{ex:X} TOPMOST={} NOACTIVATE={} owner={} z={z} fg={}",
            hwnd.0 as isize,
            ex & WS_EX_TOPMOST.0 as isize != 0,
            ex & WS_EX_NOACTIVATE != 0,
            owner.0 as isize,
            *hwnd == fg,
        );
    }
    if let Some(progman) = find_progman() {
        let z = zorder
            .iter()
            .position(|h| *h == progman)
            .map(|i| i.to_string())
            .unwrap_or_else(|| "?".to_string());
        eprintln!("[zorder]   Progman z={z}");
    }
}

/// 采集所有可见顶层窗口，按 EnumWindows 顺序（0 = 最前）
fn collect_zorder() -> Vec<HWND> {
    struct Scan {
        found: Vec<isize>,
    }
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Scan);
        if windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool() {
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

/// 采集本进程的所有便签窗口（类名 Tauri Window，exstyle 带 WS_EX_NOACTIVATE 那种以外都算）
fn collect_note_windows() -> Vec<(HWND, String)> {
    struct Scan {
        found: Vec<(isize, String)>,
    }
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Scan);
        let mut buf = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 && String::from_utf16_lossy(&buf[..len as usize]) == "Tauri Window" {
            let mut title = [0u16; 128];
            let tlen = GetWindowTextW(hwnd, &mut title);
            let text = if tlen > 0 {
                String::from_utf16_lossy(&title[..tlen as usize])
            } else {
                String::from("?")
            };
            ctx.found.push((hwnd.0 as isize, text));
        }
        true.into()
    }
    let mut ctx = Scan { found: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut ctx as *mut Scan as isize));
    }
    ctx.found
        .into_iter()
        .map(|(v, t)| (HWND(v as *mut core::ffi::c_void), t))
        .collect()
}

fn ex_style(hwnd: HWND) -> isize {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) }
}

/// 窗口此刻是否真的在置顶层（读 `WS_EX_TOPMOST`，不信任何内存状态）
pub fn is_always_on_top<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    match window.hwnd() {
        Ok(raw) => ex_style(HWND(raw.0 as *mut core::ffi::c_void)) & WS_EX_TOPMOST.0 as isize != 0,
        Err(_) => false,
    }
}

/// 把便签绑定到桌面，创建窗口后调一次（要在 `set_always_on_top` 之后）。
///
/// `pinned` 是这张便签自己的图钉状态：置顶的便签不垫底，未置顶的垫回其它窗口之下。
pub fn install<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) {
    let Ok(hwnd) = window.hwnd() else {
        eprintln!("[desktop-bind] 取窗口句柄失败，跳过桌面绑定");
        return;
    };
    let hwnd = HWND(hwnd.0 as *mut core::ffi::c_void);
    set_pinned_state(hwnd, pinned);
    debug_dump("install: 入口");

    let Some(progman) = find_progman() else {
        eprintln!("[desktop-bind] 找不到 Progman，跳过桌面绑定");
        return;
    };

    unsafe {
        let previous = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        eprintln!(
            "[desktop-bind] owner = Progman({})，绑定前 owner = {previous}",
            progman.0 as isize
        );
    }
    debug_dump("install: owner 绑定后");

    // 用户存了图钉状态就别压下去，否则会把它自己置顶的窗口又踩到最底；
    // 但**无论置顶与否**，都要把其它未置顶的便签重新垫回去——新建窗口的激活
    // 动作会把同 owner 组的兄弟窗口一起抬起来（BUG 1）。
    //
    // 层位意图的初值也在这里定：启动时未置顶的便签**仍然**要垫底（否则一开机便签
    // 就盖住浏览器），所以是 `Sunk`；置顶的本身就在最前，记成 `Raised`。
    if pinned {
        mark_raised(hwnd);
    } else {
        mark_sunk(hwnd);
        park_behind(hwnd);
    }
    // 校验要在垫底之前做：置顶的窗口不该被压下去，而垫底会把它自己踩到底。
    verify_and_fix_topmost(window, pinned);
    // 新建窗口的创建/显示本身也可能带起兄弟窗口，最后统一压一遍
    park_siblings_behind(hwnd);
    // 顺带补回被连带清掉的兄弟 TOPMOST 位（BUG 3 的同一机制）
    repair_pinned_topmost(hwnd);
    debug_dump("install: 结束");
}

/// 用户按图钉切换置顶时同步状态。
///
/// 调用顺序必须是"先 `window.set_always_on_top(pinned)`，再调本函数"，因为
/// tao 取消置顶用的是 `HWND_NOTOPMOST`，会把窗口顶到普通层最前面。
///
/// **0.4.1 起取消置顶不再 `park_behind`**（BUG A）：用户点掉图钉时窗口应该"停在原处、
/// 只是不再压住别的置顶内容"，而不是直接沉到所有窗口之下（那样看起来像消失了）。
/// tao 的 `HWND_NOTOPMOST` 自带"落到普通层最上面"的效果，正好就是这个位置。
///
/// 启动时（`install()`）未置顶的便签仍然要垫底，那条规则没变。
///
/// 返回"实际生效的置顶状态"（回读 exstyle），供前端校准图标。
pub fn set_user_pinned<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) -> bool {
    let Ok(raw) = window.hwnd() else {
        return false;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    set_pinned_state(hwnd, pinned);
    debug_dump("set_user_pinned: 入口");

    // 1) 两种切换都意味着"这张此刻在前面"：置顶本来就在前，取消置顶则停在原处。
    //    标记之后 `park_siblings_behind` 就不会把这张再踩回去。
    mark_raised(hwnd);

    // 2) 回读 exstyle 校验，必要时重试——这一步可能再次触发 HWND_TOPMOST
    let _ = verify_and_fix_topmost(window, pinned);

    // 3) 关键：把**其它**未置顶便签中仍处于 `Sunk` 的那些压回去。激活/置顶动作会把同一
    //    owner 组的兄弟窗口一起抬到普通层最前面，不补这一刀就会"点 A 的图钉，
    //    B 窜到前台应用之上"。已经 `Raised` 的兄弟不动（它们本来就在前面）。
    //    必须放在最后，因为第 2 步的重试也可能带起兄弟窗口。
    park_siblings_behind(hwnd);

    // 3.5) BUG 3 的补丁：tao 取消置顶时用的 HWND_NOTOPMOST 不带 SWP_NOOWNERZORDER，
    //      会把兄弟置顶窗口的 TOPMOST 位一起带走，这里回读并按需补回去。
    repair_pinned_topmost(hwnd);

    // 4) 以最终的真实 exstyle 为准回报（不轮询，只在切换路口读一次）
    let actual = is_always_on_top(window);
    if debug_enabled() {
        eprintln!("[zorder] set_user_pinned: 期望 pinned={pinned}，实际 topmost={actual}");
    }
    debug_dump("set_user_pinned: 结束");
    actual
}

/// 回读 `WS_EX_TOPMOST`，与期望不一致就重试一次；返回**实际**状态。
///
/// 这一层是给"图标说钉住、实际没钉住"兜底的：不轮询，只在状态切换的路口校验一次。
fn verify_and_fix_topmost<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) -> bool {
    if !pinned {
        // 取消置顶时，只要 topmost 位已经没了就算成功（垫底由调用方负责）
        if is_always_on_top(window) {
            let _ = window.set_always_on_top(false);
            if debug_enabled() {
                eprintln!("[zorder] 校验：取消置顶后仍带 TOPMOST，重试一次");
            }
        }
        let actual = is_always_on_top(window);
        if debug_enabled() && actual {
            eprintln!("[zorder] 校验：重试后仍然 TOPMOST，如实回报");
        }
        return actual;
    }

    if is_always_on_top(window) {
        return true;
    }
    if debug_enabled() {
        eprintln!("[zorder] 校验：期望置顶但 exstyle 无 TOPMOST，重试一次");
    }
    let _ = window.set_always_on_top(true);
    // tao 的 set_always_on_top 走 SwpFlags；同线程时立即生效，这里读一次确认
    if !is_always_on_top(window) {
        // 仍不一致就直接兜底一次 HWND_TOPMOST（不轮询、只在切换路口发生）
        if let Ok(raw) = window.hwnd() {
            let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
                );
            }
        }
    }
    let actual = is_always_on_top(window);
    if debug_enabled() && !actual {
        eprintln!("[zorder] 校验：重试后仍未置顶，如实回报 false");
    }
    actual
}

/// 垫到 owner 之上、其它窗口之下。
///
/// **必须带 `SWP_NOOWNERZORDER`**：不带的话这一步会把同 owner 组里其它便签的
/// `WS_EX_TOPMOST` 一起清掉（BUG 2 的直接原因，实测 0x40118 -> 0x40110）。
fn park_behind(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// 把所有"该沉在底部"的便签重新垫回底部；`keep` 是当前正在操作的那张（它自己已落位）。
///
/// **只处理 `pinned == false` 且层位意图为 `Sunk` 的兄弟窗口**：
/// - 置顶的不动（它本来就在置顶层）；
/// - 已被用户显式抬到前面的（`Raised`）不动，否则"点托盘全部抬到最前"会被随后的
///   `Focused(true)` 事件立刻踩回去（BUG B 的"闪一下又回到后面"）。
///
/// 带 `SWP_NOACTIVATE` 保证不抢焦点、不带起额外的激活连带。
fn park_siblings_behind(keep: HWND) {
    let progman = find_progman();
    for (hwnd, title) in collect_note_windows() {
        if hwnd == keep {
            continue;
        }
        // 只处理真正属于我们这套桌面绑定的窗口（owner = Progman）
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
        if progman.is_none() || Some(owner) != progman {
            continue;
        }
        if is_user_pinned(hwnd) || ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0 {
            continue;
        }
        // 0.4.1：被显式抬到前面的兄弟不踩回去
        if layer_intent(hwnd) == LayerIntent::Raised {
            if debug_enabled() {
                eprintln!("[zorder]   跳过已抬起的兄弟窗口 {title} (hwnd={})", hwnd.0 as isize);
            }
            continue;
        }
        if debug_enabled() {
            eprintln!("[zorder]   park 兄弟窗口 {title} (hwnd={})", hwnd.0 as isize);
        }
        park_behind(hwnd);
    }
}

/// 激活某个便签之后补一刀：把所有**其它**未置顶、且意图仍为 `Sunk` 的便签压回底部。
///
/// 为什么必须有这一步：激活 owner = Progman 的窗口时，Windows 会把同一 owner 组的
/// 兄弟窗口一起抬到普通层最前面（实测 B 从 z=28 直接跳到 z=7，越过前台浏览器）。
/// `SWP_NOOWNERZORDER` 管不了"激活"，只能在激活之后再垫一次。
///
/// 0.4.1 补充：被激活的这张自己标记为 `Raised`（用户/程序刚把它拿到前面），
/// 于是这里只会压"后台还沉在底部"的兄弟。**不需要**额外的全局抑制标志。
pub fn park_siblings_after_activation<R: Runtime>(activated: &WebviewWindow<R>) {
    let Ok(raw) = activated.hwnd() else {
        return;
    };
    let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
    debug_dump("激活前");
    mark_raised(hwnd);
    park_siblings_behind(hwnd);
    debug_dump("激活后补垫");
}

/// 把尚未置顶的便签全部重新垫底、并把层位意图复位成 `Sunk`（`--sink-notes` 用）。
///
/// 供回归脚本把状态复位到"开机默认"：之后自动沉降逻辑才会重新对这些窗口生效。
/// 置顶的便签不参与（它们不该被压下去），它们的意图保持 `Raised`。
pub fn sink_all_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    let mut sunk = 0;
    for window in windows {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        if is_user_pinned(hwnd) || ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0 {
            mark_raised(hwnd);
            continue;
        }
        mark_sunk(hwnd);
        park_behind(hwnd);
        sunk += 1;
    }
    // 垫底动作会牵动同一 owner 组，最后统一把"仍该沉底"的再压一遍
    for window in windows {
        if let Ok(raw) = window.hwnd() {
            park_siblings_behind(HWND(raw.0 as *mut core::ffi::c_void));
        }
    }
    debug_dump("sink_all_notes: 结束");
    sunk
}

/// 把**所有**便签显式抬到前台应用之上（托盘左键 / `--raise-all` 用）。
///
/// `windows` 按 store 里的顺序传进来（后建的在前），本函数**从后往前**处理：
/// 先抬最后一张，再把更靠前的依次抬到 `HWND_TOP`，于是传入顺序的第一张最终在最前面。
///
/// 每张窗口的动作：
/// 1. `mark_raised(hwnd)`——先写意图，之后紧跟的 `Focused(true)` 触发
///    `park_siblings_behind` 时就不会把兄弟们踩回去（BUG B 的根因）；
/// 2. `SetWindowPos(hwnd, HWND_TOP, …, SWP_NOACTIVATE|SWP_NOOWNERZORDER)`；
///    置顶的那些用 `HWND_TOPMOST`，保持它们在置顶层的相对顺序。
///
/// 最后只对**最前面那一张**调一次 `set_focus()`：整排互相抢焦点会引发一串
/// `Focused` 事件和可见的闪烁。
///
/// 返回实际抬起的窗口数。
pub fn raise_notes<R: Runtime>(windows: &[WebviewWindow<R>]) -> usize {
    let mut raised = 0;
    let mut front: Option<&WebviewWindow<R>> = None;

    for window in windows.iter().rev() {
        let Ok(raw) = window.hwnd() else {
            continue;
        };
        let hwnd = HWND(raw.0 as *mut core::ffi::c_void);
        mark_raised(hwnd);
        // 置顶的窗口抬回置顶层，未置顶的抬到普通层最前；两者都带 NOACTIVATE，
        // 避免抬一张就把焦点拽走（焦点最后统一给一次）。
        let after = if is_user_pinned(hwnd) || ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0 {
            HWND_TOPMOST
        } else {
            HWND_TOP
        };
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
        raised += 1;
        front = Some(window);
    }

    // 只聚焦一次，且聚焦的是"最后被抬到最上面"的那张（即传入顺序的第一张）
    if let Some(window) = front {
        let _ = window.set_focus();
    }
    debug_dump("raise_notes: 结束");
    raised
}

/// 把"本该置顶却丢了 `WS_EX_TOPMOST`"的兄弟窗口重新钉回置顶层（BUG 3 的补丁）。
///
/// 判定依据是 `USER_PINNED` 里的用户意图（`install` / `set_user_pinned` 都会写），
/// 所以只补真正的置顶窗口，不会把用户取消掉的置顶又钉回去。
/// 全程带 `SWP_NOACTIVATE | SWP_NOOWNERZORDER`，不抢焦点、不牵动 owner 组。
fn repair_pinned_topmost(keep: HWND) {
    let progman = find_progman();
    for (hwnd, title) in collect_note_windows() {
        if hwnd == keep {
            continue;
        }
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) }.unwrap_or_default();
        if progman.is_none() || Some(owner) != progman {
            continue;
        }
        if !is_user_pinned(hwnd) || ex_style(hwnd) & WS_EX_TOPMOST.0 as isize != 0 {
            continue;
        }
        if debug_enabled() {
            eprintln!("[zorder] 修复兄弟窗口 {title} 的 TOPMOST 位（BUG 3）");
        }
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

/// 安全地把窗口从置顶层撤下来（不做激活、不影响兄弟窗口）
#[allow(dead_code)]
fn clear_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_NOTOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// 枚举找桌面窗口：`FindWindow("Progman", None)` 在本机返回 0，只能靠 EnumWindows。
fn find_progman() -> Option<HWND> {
    struct Scan {
        found: isize,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lparam.0 as *mut Scan);
        let mut buf = [0u16; 128];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]);
            if class == "Progman" {
                ctx.found = hwnd.0 as isize;
                return false.into();
            }
        }
        true.into()
    }

    let mut ctx = Scan { found: 0 };
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut ctx as *mut Scan as isize));
    }
    if ctx.found == 0 {
        None
    } else {
        Some(HWND(ctx.found as *mut core::ffi::c_void))
    }
}
