//! 设置面板：一个**独立的小窗口**，不是便签窗口里的遮罩。
//!
//! 为什么不把面板画在便签窗口里：
//! 1) 便签可能只有 40px 高（折叠态）或 240px 宽（最窄），面板塞不进去就只能加滚动条，
//!    而滚动条在这么小的面板里极其难用；
//! 2) 若改成"把便签窗口临时放大、面板画在透明留白上"，那圈透明留白**照样捕获鼠标**。
//!    `pointer-events: none` 只影响 DOM，不影响窗口；真正要穿透得用
//!    `set_ignore_cursor_events`，可它是**整窗开关**，只能跟着鼠标位置反复切换，
//!    结果就是鼠标移到面板上要先被穿透、晃一下才响应，手感很差。
//!
//! 独立窗口没有透明留白，以上问题一次都不存在：面板本身就是窗口，想多大都行，
//! 折叠成条时也能完整展开，且永远不需要滚动条。
//!
//! 面板窗口的 label 是 `settings-<noteId>`；命令入口统一通过 `note_id_from_label`
//! 把 `settings-*` 归属回 `note-*`，所以面板里发起的外观 / 标题 / 隐藏 / 删除
//! 与在便签窗口里发起走的是同一套后端逻辑。

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::events;

/// 设置面板窗口 label 前缀，完整形式 `settings-<noteId>`
pub const LABEL_PREFIX: &str = "settings-";

/// 面板尺寸（逻辑像素）。
///
/// 高度是**按内容量定死**的：窗口就是面板，不留可滚动区域，
/// 所以内容增减时这个数字要跟着调（底部多出来的空白一眼就能看出来）。
pub const WIDTH: f64 = 300.0;
pub const HEIGHT: f64 = 376.0;

/// 面板与便签之间的间隙（逻辑像素）
const GAP: f64 = 8.0;

fn label(note_id: &str) -> String {
    format!("{LABEL_PREFIX}{note_id}")
}

/// 从面板窗口的 label 反查所属便签 id
pub fn note_id_from_label(label: &str) -> Option<String> {
    label
        .strip_prefix(LABEL_PREFIX)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

/// 打开（或聚焦）某张便签的设置面板。
///
/// `note_id` 是面板所代表的便签；`parent` 是那张便签的窗口，用来定位和跟随。
pub fn open(app: &AppHandle, note_id: &str, parent: &WebviewWindow) -> Result<(), String> {
    let label = label(note_id);

    // 已经开着就只抬到最前，不重建（否则外观会闪一下）
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.show();
        let _ = existing.set_focus();
        events::settings_state(app, note_id, true);
        return Ok(());
    }

    let note_pinned = parent.is_always_on_top().unwrap_or(false);

    let panel = WebviewWindowBuilder::new(
        app,
        &label,
        WebviewUrl::App(format!("index.html?settings={note_id}").into()),
    )
    .title("便签设置")
    .inner_size(WIDTH, HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    // 透明只为圆角的抗锯齿：面板铺满整个窗口，没有可用于交互的透明区域，
    // 所以不存在"透明区要不要穿透鼠标"的问题。
    .transparent(true)
    .skip_taskbar(true)
    // 层级策略：面板**不**无条件置顶，只在便签置顶时跟着置顶。
    // 便签没置顶时面板就是普通窗口，被别的应用盖住是正常行为——
    // 之前这里写死 `always_on_top(true)`，导致"便签没钉住、面板却永远飘在最上面"。
    .always_on_top(note_pinned)
    .shadow(false)
    .focused(true)
    .build()
    .map_err(|e| format!("创建设置面板失败：{e}"))?;

    // 初次定位：此时窗口已建好，才能读到它的真实尺寸
    let _ = panel.set_position(
        position_for(parent, &panel).map_err(|e| format!("计算面板位置失败：{e}"))?,
    );

    // 面板被关闭（用户点 X / Esc / 系统关闭）时，通知便签窗口同步齿轮高亮。
    // 这里**不能** prevent_close：面板关掉就该销毁，下次打开重新读状态。
    let app_closed = app.clone();
    let note_id_closed = note_id.to_string();
    panel.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            events::settings_state(&app_closed, &note_id_closed, false);
        }
    });

    events::settings_state(app, note_id, true);
    Ok(())
}

/// 面板是否开着（按窗口是否存在判断，这是唯一可信的状态源）
pub fn is_open(app: &AppHandle, note_id: &str) -> bool {
    app.get_webview_window(&label(note_id)).is_some()
}

/// 关掉某张便签的面板（如果开着）。销毁窗口而不是隐藏，下次打开总是新状态。
pub fn close(app: &AppHandle, note_id: &str) {
    if let Some(panel) = app.get_webview_window(&label(note_id)) {
        let _ = panel.destroy();
    }
    // 销毁回调还会再发一次，这里补一次保证「面板本来就没开」时状态也对
    events::settings_state(app, note_id, false);
}

/// 按内容高度调整面板窗口。
///
/// 高度由前端测量内容后上报——写死一个常量迟早对不上（字号、标题换行、
/// 控件增删都会改变内容高度），结果不是裁掉底部就是留大片空白。
/// 这里只管把数字夹进合理区间并换成物理像素。
pub fn resize_to_content(app: &AppHandle, note_id: &str, content_height: f64) {
    let Some(panel) = app.get_webview_window(&label(note_id)) else {
        return;
    };
    let Ok(scale) = panel.scale_factor() else {
        return;
    };

    // 夹一个下限防止内容还没渲染时量到 0；上限防止异常值把面板撑爆
    let logical = content_height.max(120.0).min(2000.0);
    let height = (logical * scale).round() as u32;
    let Ok(current) = panel.inner_size() else {
        return;
    };

    // 高度没变就别折腾窗口，避免无谓的重绘 / 闪烁
    if current.height.abs_diff(height) <= 1 {
        return;
    }

    let _ = panel.set_size(tauri::PhysicalSize::new(current.width, height));
    // 尺寸变了位置也要重算（底部可能超出屏幕）
    follow(app, note_id);
}

/// 便签移动 / 缩放了：面板跟着挪，别飘在原地。
///
/// 挂在窗口的 `Moved` / `Resized` 事件上**立即执行**，不走 400ms 的落盘防抖——
/// 否则拖动便签时面板不动，松手瞬间才瞬移过去。
/// 每次只做一次位置计算 + 一次 `set_position`，开销可以忽略。
pub fn follow(app: &AppHandle, note_id: &str) {
    let Some(panel) = app.get_webview_window(&label(note_id)) else {
        return;
    };
    let Some(parent) = app.get_webview_window(&format!("note-{note_id}")) else {
        return;
    };
    if let Ok(position) = position_for(&parent, &panel) {
        let _ = panel.set_position(position);
    }
}

/// 便签置顶状态变了：面板层级跟着变，否则一个置顶一个不置顶会互相盖住。
pub fn sync_layer(app: &AppHandle, note_id: &str, pinned: bool) {
    if let Some(panel) = app.get_webview_window(&label(note_id)) {
        let _ = panel.set_always_on_top(pinned);
    }
}

/// 面板贴边定位。
///
/// 策略（按优先级）：
/// 1. **便签右侧**（首选，面板不压住便签）；
/// 2. **便签左侧**（右侧放不下时翻边）；
/// 3. **盖在便签上**——两侧都放不下（便签贴屏幕边、或便签比屏幕还宽）时，
///    宁可让面板压着便签，也不要让它伸出屏幕再被拽回来。
///
/// 第 3 条是修 bug 加的：原来 else 分支仍选 `right_x`，面板伸出屏幕后会被
/// `clamp_to_area` 猛拽回边缘，表现为"一拖到屏幕边，面板就跳到奇怪的地方"。
/// 现在的做法是**先夹好再决定**，任何分支出来的坐标都已经在工作区内。
///
/// 竖直方向与便签顶部对齐，再整体夹进屏幕，所以折叠态（40px 高）也不会把面板顶出去。
fn position_for(parent: &WebviewWindow, panel: &WebviewWindow) -> tauri::Result<PhysicalPosition<i32>> {
    let scale = parent.scale_factor()?;
    let gap = (GAP * scale).round() as i32;

    // 面板尺寸取**窗口真实值**，不用常量：高度是随内容变的
    let (width, height) = match panel.inner_size() {
        Ok(size) => (size.width as i32, size.height as i32),
        Err(_) => (
            (WIDTH * scale).round() as i32,
            (HEIGHT * scale).round() as i32,
        ),
    };

    let (Ok(position), Ok(size)) = (parent.outer_position(), parent.outer_size()) else {
        return Ok(PhysicalPosition::new(0, 0));
    };

    let monitors = parent.available_monitors().unwrap_or_default();
    let center_x = position.x + size.width as i32 / 2;
    let center_y = position.y + size.height as i32 / 2;
    // 用 work_area_for 而不是 work_area_at：便签可能被拖到屏幕外，
    // 那时要退到最近的显示器，不能用编造的默认工作区（会导致面板跳到固定位置）
    let area = crate::work_area_for(&monitors, center_x, center_y);

    // 便签中心是否落在某块显示器内。不在就说明它被拖到屏幕外了。
    let parent_on_screen = crate::work_area_at(&monitors, center_x, center_y).is_some();

    let right_x = position.x + size.width as i32 + gap;
    let left_x = position.x - width - gap;

    if !parent_on_screen {
        /*
          便签在屏幕外：**不要**把面板夹回屏幕内。
          夹回来的结果就是"面板跳到某个固定位置、和便签彻底分离"（用户报的 bug）——
          因为兜底工作区是编造的默认值，面板每次都落到同一个点。

          这里让面板跟着便签一起出去，保持相对关系；用户把便签拖回屏幕时，
          面板也一起回来。位置纯粹由便签位置推导，不含任何屏幕裁剪，所以可预期。
        */
        return Ok(PhysicalPosition::new(right_x, position.y));
    }

    let (ax, ay, aw, ah) = area;
    let work_right = ax + aw as i32;
    let work_bottom = ay + ah as i32;

    // 三条候选各自先夹进工作区，再挑一条真正不会"越界后被猛拽"的
    let x = if right_x + width <= work_right {
        right_x
    } else if left_x >= ax {
        left_x
    } else {
        // 两侧都放不下：盖在便签上，并保持面板左边缘不越界
        position.x.min(work_right - width).max(ax)
    };

    let (x, y) = crate::clamp_to_area(area, x, position.y, width as u32, height as u32);
    // clamp 只兜底；上面的分支已经保证了在界内，这里再夹一次是为了防显示器切换等极端情况
    let x = x.min(work_right - width).max(ax);
    let y = y.min(work_bottom - height).max(ay);
    Ok(PhysicalPosition::new(x, y))
}
