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
//! 所以启动时和取消图钉时都垫一次。
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

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, SetWindowLongPtrW, SetWindowPos, GWLP_HWNDPARENT, HWND_BOTTOM,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
};

/// 每个便签窗口（HWND）各自的图钉状态
static USER_PINNED: Mutex<Option<HashMap<isize, bool>>> = Mutex::new(None);

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

/// 把便签绑定到桌面，创建窗口后调一次（要在 `set_always_on_top` 之后）。
///
/// `pinned` 是这张便签自己的图钉状态：置顶的便签不垫底，未置顶的垫回其它窗口之下。
pub fn install<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) {
    let Ok(hwnd) = window.hwnd() else {
        eprintln!("[desktop-bind] 取窗口句柄失败，跳过桌面绑定");
        return;
    };
    set_pinned_state(hwnd, pinned);

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

    // 用户存了图钉状态就别压下去，否则会把它自己置顶的窗口又踩到最底
    if !pinned {
        park_behind(hwnd);
    }
}

/// 用户按图钉切换置顶时同步状态；取消置顶后把便签重新垫回桌面之上。
///
/// 调用顺序必须是"先 `window.set_always_on_top(pinned)`，再调本函数"，因为
/// tao 取消置顶用的是 `HWND_NOTOPMOST`，会把窗口顶到普通层最前面。
///
/// **任何把窗口从置顶降下来的路径都必须走到这里**（`pinned == false` 时补一次
/// `HWND_BOTTOM`），否则窗口会停留在前台应用之上，表现为"后台便签自己跳出来"。
pub fn set_user_pinned<R: Runtime>(window: &WebviewWindow<R>, pinned: bool) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    set_pinned_state(hwnd, pinned);
    if pinned {
        return;
    }
    park_behind(hwnd);
}

/// 垫到 owner 之上、其它窗口之下
fn park_behind(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
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
