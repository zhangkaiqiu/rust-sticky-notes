//! 效果 B 实验：把便签"绑定到桌面"（`owner = Progman`）。
//!
//! ## 做法
//! `SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman)` 把桌面窗口设成便签的 owner。
//! owner 窗口永远在它的 owner 之上，所以：
//! - 平时：前台应用盖住便签（普通 Z 序，可被覆盖，符合要求）；
//! - Win+D：shell 把应用最小化、把 Progman 抬起来，便签因为"在 owner 之上"自然
//!   浮在桌面上，不需要任何置顶操作；
//! - 再按 Win+D：应用恢复，便签重新被盖住。
//!
//! ## 本版刻意保持最小
//! 上一版把 owner 绑定和 `SetWinEventHook` 临时置顶、`-32000` 窗口子类化混在一起，
//! 出现"第二次 Win+D 不恢复之前的窗口"。这一版**只做 owner 绑定**，不带任何钩子、
//! 不带置顶、不带子类化，用来把"owner 本身有问题"和"附加机制互相打架"区分开。
//!
//! 保留的对外 API 与上一版一致（`install` / `set_user_pinned`），所以 `lib.rs` 不用改。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, SetWindowLongPtrW, GWLP_HWNDPARENT,
};

static NOTE_HWND: AtomicIsize = AtomicIsize::new(0);
/// 用户按图钉手动置顶的状态（本方案下只记录，不参与 Z 序逻辑）
static USER_PINNED: AtomicBool = AtomicBool::new(false);

/// 把便签绑定到桌面，setup 里调一次。
pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(hwnd) = window.hwnd() else {
        eprintln!("[desktop-bind] 取窗口句柄失败，跳过桌面绑定");
        return;
    };
    NOTE_HWND.store(hwnd.0 as isize, Ordering::Release);

    let Some(progman) = find_progman() else {
        eprintln!("[desktop-bind] 找不到 Progman，跳过桌面绑定");
        debug_log("找不到 Progman，跳过桌面绑定");
        return;
    };

    unsafe {
        let previous = SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman.0 as isize);
        debug_log(&format!(
            "owner 设成 Progman({}) 成功，改前 owner={previous}",
            progman.0 as isize
        ));
    }
}

/// 用户按图钉切换置顶时同步状态。
pub fn set_user_pinned(pinned: bool) {
    USER_PINNED.store(pinned, Ordering::Release);
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
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut ctx as *mut Scan as isize),
        );
    }
    if ctx.found == 0 {
        None
    } else {
        Some(HWND(ctx.found as *mut core::ffi::c_void))
    }
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
