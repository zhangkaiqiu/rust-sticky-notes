# rust-sticky-notes

Windows 桌面便签（Tauri 2 + Rust + React + TypeScript + Vite + Tailwind CSS）。

**每张便签一个独立窗口**，支持 6 套配色、3 档字号、深浅主题与可调透明度；基于
`owner = Progman` 的桌面绑定实现 Win+D 抵抗，同时保留"平时会被前台应用正常覆盖"的层级。
`npm install` 后 `npm run tauri dev` 调试、`npm run tauri build -- --no-bundle` 出 exe，
详见[运行](#运行)与[打包](#打包)。采用 MIT 许可证，详见 [LICENSE](LICENSE)。

## 技术栈

| 层 | 选型 |
| --- | --- |
| 外壳 | Tauri 2（Rust） |
| 前端 | React 18 + TypeScript + Vite 6 |
| 样式 | Tailwind CSS 3 |
| 持久化 | 单个 JSON 文件，`serde` + `serde_json`（不用数据库、不用 plugin-store） |
| 自启 | `tauri-plugin-autostart` v2（HKCU Run 键） |
| 托盘 | Tauri 2 内置 `TrayIconBuilder` |
| Win32 交互 | `windows` 0.62（版本与 tauri 2.12 内部依赖一致，复用同一份 crate，不额外增加编译量） |

## 目录结构

```
rust-sticky-notes/
├── index.html                  # 透明背景的挂载页
├── package.json
├── vite.config.ts              # 固定 1420 端口给 tauri dev 用
├── tailwind.config.js
├── postcss.config.js
├── tsconfig.json
├── LICENSE                     # MIT
├── scripts/gen_icon.py         # 用 Pillow 生成 1024px 源图标（一次性脚本）
├── src/                        # 前端
│   ├── main.tsx
│   ├── App.tsx                 # 单窗口状态编排 + 防抖保存（每个窗口只管自己那张便签）
│   ├── api.ts                  # invoke 封装
│   ├── theme.ts                # 6 套配色 × 浅/深、字号、透明度 → CSS 变量
│   ├── types.ts                # 与 Rust store 结构对应
│   ├── styles.css              # Tailwind + 主题变量 / 滚动条 / 滑块
│   └── components/
│       ├── TopBar.tsx          # 拖动区 + 可编辑标题 + 图钉（旋转 45°）+ 设置
│       ├── TodoList.tsx        # 待办增删改勾选
│       ├── SettingsPanel.tsx   # 配色/字号/主题/透明度 + 置顶/自启/隐藏/删除/退出
│       └── icons.tsx           # 内联 SVG，无图标库依赖
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json         # 不在配置里预建窗口，全部由 Rust 按便签创建
    ├── capabilities/default.json
    ├── icons/                  # 由 `tauri icon` 生成
    └── src/
        ├── main.rs
        ├── lib.rs              # 命令 + 每便签窗口 + 托盘 + 几何持久化
        ├── store.rs            # store.json v2 读写（v1 迁移 + 原子替换）
        └── win_desktop_pin.rs  # 抵抗 Win+D（owner = Progman 桌面绑定）
```

## 运行

首次需要 Node 18+、Rust 1.77+（stable-msvc）、MSVC Build Tools、WebView2 Runtime（Win11 自带）。

```powershell
npm install
npm run tauri dev      # 开发模式（Vite HMR + Rust 热重启）
```

只调前端时也可以 `npm run dev`（浏览器里看不到托盘、置顶等原生能力）。

## 打包

```powershell
npm run tauri build -- --no-bundle
```

产物：

- 可执行文件：`src-tauri/target/release/rust-sticky-notes.exe`
- 安装包（去掉 `--no-bundle` 时）：`src-tauri/target/release/bundle/nsis/Rust Sticky Notes_0.2.0_x64-setup.exe`

**必须走 Tauri CLI**：直接 `cargo build --release` 不会带上 `custom-protocol` 特性，
打出来的 exe 会去连 `localhost:1420`，显示"无法访问此页面"。

NSIS 安装模式为 `currentUser`（不需要管理员权限）。

## 已实现

- **多便签**：`notes[]` 里每条一个窗口，`label = note-<id>`；托盘「新建便签」按
  `settings.defaultStyle` 初始化，新窗口相对已有窗口层叠偏移（每张 +32 逻辑像素），并夹回可见工作区
- 窗口：`decorations: false` / `transparent: true` / `skipTaskbar: true` / 默认不置顶，
  初始 320×420（物理像素按 DPI 换算），可拖动（顶部 `data-tauri-drag-region`）、可拉伸
- 便签文本：textarea，停止输入 450ms 后 `invoke("save_note_content")` 落盘，只写当前窗口对应的那张
- **标题可编辑**：点顶部标题原地变输入框（不是弹窗），回车 / 失焦保存，`Esc` 取消；
  空标题回落到「便签」，输入上限 60 字符，过长单行省略并带 `title` 提示；
  标题区之外仍是拖动区，输入框不参与拖拽
- TODO：添加（回车）、勾选完成、行内改文字、删除、批量清除已完成，与文本一起持久化
- **外观（按便签独立）**：
  - 配色：奶油 / 薄荷 / 樱粉 / 天蓝 / 沙金 / 石墨，每套都有浅色与深色两份色板
  - 字号：小 / 中 / 大（基准 14 / 16 / 18 px），由 CSS 变量 `--note-font-size` 驱动，
    正文、TODO、输入框按 em 等比缩放，不用 `transform: scale`
  - 主题：浅色 / 深色；深色下卡片底色走 `#2A2622` 一类的深色系，文字、边框、分隔线、
    滚动条、滑块全部跟随
  - **透明度**：35%–100%（默认 97%，视觉与改造前一致），只作用于卡片底色和边框的 alpha，
    文字 / 图标 / 勾选态 alpha 不变；低于 60% 时补一圈极轻的内描边帮助辨认边界
  - 设置面板里的滑块拖动实时预览，停手 300ms 后落盘
- **图钉**：置顶时图钉整体旋转 45° 并加深颜色（无外圈、无底色块），未置顶时竖直淡色、
  hover 变深；每张便签的 `pinned` 状态独立保存
- 设置面板（便签内小浮层，不另开窗口）：配色 / 字号 / 主题 / 透明度 / 置顶 / 开机自启 /
  隐藏到托盘 / 删除此便签（二次确认）/ 退出应用 / 数据文件路径
- 托盘：右键菜单顶部是**便签列表**（每张一行，点击唤回并聚焦；最多 20 张，超出显示「…另有 N 张」），
  分隔线后是「新建便签 / 显示全部便签 / 隐藏全部便签 / 退出」，左键单击唤回全部便签
- 窗口位置与大小：按便签保存；监听 `Moved` / `Resized`，同一窗口 400ms 静默后写盘；
  启动时恢复并夹回所在显示器工作区，显示器不存在则回主屏居中
- 开机自启：设置面板开关直连 `tauri-plugin-autostart`，每次启动以系统真实状态为准回写 JSON
- 关窗即隐藏（`CloseRequested` 里 `prevent_close` + `hide` + `visible = false`），进程常驻托盘
- **单实例锁**：`tauri-plugin-single-instance`（在 `Builder` 上第一个注册）。第二次双击 exe 不再起第二套窗口，
  而是把已有便签 `show()` + `set_focus()`（第一张获得焦点）；判定按应用 identifier，
  因此**便携模式的两份拷贝同机运行时，第二份会被判定为"已有实例"**
- **删除便签**：Rust 侧先从 `notes[]` 移除并落盘，再销毁窗口；删除最后一张后刷盘并退出应用
- **并发写安全**：所有写操作都经过 Rust 侧单一的 `Mutex<Store>`，持锁完成"改内存 + 原子落盘"
  （临时文件 + `rename`），前端不直接写文件，两个窗口不会互相覆盖
- 退出：托盘退出、设置面板退出、`RunEvent::ExitRequested` 都会先 flush 再退出
- **抵抗 Win+D**：每个便签窗口各自调用 `win_desktop_pin::install(&window, pinned)`（机制不变，
  见下文「抵抗 Win+D」一节）；取消图钉时调用 `set_user_pinned(&window, false)` 垫回其它窗口之下

## 数据文件与迁移

路径：`%APPDATA%\com.ruststicky.notes\store.json`

当前格式为 **version 2**：

```json
{
  "version": 2,
  "settings": {
    "autostart": true,
    "defaultStyle": { "color": "cream", "theme": "light", "fontSize": "md", "opacity": 0.97 }
  },
  "notes": [
    {
      "id": "9c1f7e02-...",
      "title": "便签",
      "text": "正文",
      "todos": [{ "id": "8a2b...", "text": "买牛奶", "done": false }],
      "color": "cream",
      "theme": "light",
      "fontSize": "md",
      "opacity": 0.97,
      "pinned": false,
      "visible": true,
      "bounds": { "x": 0, "y": 0, "width": 640, "height": 840 }
    }
  ]
}
```

- `notes` 是数组，标题 / 内容 / 外观 / 置顶 / 可见性 / 几何全部按便签独立保存
- `title` 上限 60 个字符，全空白会回落为「便签」，并同步到操作系统窗口标题（Alt+Tab 可见）
- `fontSize` 取 `sm | md | lg`；`color` 取 `cream | mint | rose | sky | sand | graphite`；
  `theme` 取 `light | dark`；`opacity` 会被 clamp 到 0.35–1
- `bounds` 是**物理像素**；`visible: false` 的便签启动时不建窗口，托盘「显示全部」会重建

**从 v1（单便签）迁移**：

- 启动时检测 JSON 里有没有 `notes` 数组：没有就按旧格式 `{ text, todos, settings, window[, style] }` 解析
- 迁移前先把原文件备份成同目录的 `store.v1.bak.json`（备份已存在则不覆盖，便于回溯）
- 旧便签的 `text` / `todos` / `window` → 新 `notes[0]` 的内容与 `bounds`，标题固定为「便签」；
  `settings.alwaysOnTop` → `notes[0].pinned`；`settings.autostart` → 新 `settings.autostart`；
  若旧数据里已有 `style`（任务二版本）则继承，否则用默认奶油/浅色/中/97%
- 迁移成功后立即按 v2 落盘，因此重复启动不会再走迁移分支（幂等），也不会重复备份

## 便携模式

默认数据放在 `%APPDATA%\com.ruststicky.notes\store.json`。如果想让数据跟着程序走（U 盘、绿色版）：

1. 把 `rust-sticky-notes.exe` 放到一个有写权限的目录；
2. 在 exe 同目录新建一个空的 `portable.txt`（文件名固定，内容不校验）；
3. 启动程序。启动日志会打印 `[store] 便携模式：...` 和 `[store] 数据文件: <绝对路径>`。

行为：

- 便携模式下 `store.json` 写在 **exe 同目录**（和 `portable.txt` 并排），v1 迁移产生的
  `store.v1.bak.json` 也在同一目录；
- **首次**便携启动时，如果 exe 同目录没有 `store.json` 而 AppData 有，会复制一份过去，避免切换模式丢数据；
- 删掉 `portable.txt` 再启动就回到 AppData 那份数据。**不做反向同步，也不删除任何文件**，两边各自独立；
- 多窗口写盘仍然只有一把锁（Rust 侧唯一的 `Mutex<Store>`），路径变了但并发安全不变。

注意：

- 便携目录必须可写（放在 `Program Files` 之类只读目录会写盘失败）；
- exe 和 `store.json` 要一起搬走；只搬 exe 会得到一份全新的空便签；
- 便携模式下开机自启注册的仍是**当前 exe 的绝对路径**：exe 被移动或改名后自启会失效，需要重新开关一次；
- 便携数据和 AppData 数据完全独立，切换模式前建议先备份 `store.json`；
- 单实例锁按应用 identifier 判定（不是按 exe 路径），所以**便携模式的两份拷贝同机运行**时，
  第二份会被当成"已有实例"直接退出，并把第一份的便签唤到前台——想在两台机上各跑一份只能放不同机器

## 托盘菜单

右键托盘图标弹出的菜单顶部是**便签列表**：每张便签一行，显示它的标题（默认标题的显示为
「便签 1」「便签 2」…），点击即 `show()` + `set_focus()` 把那张便签唤到前台；选中的便签若窗口
还不存在（启动时是隐藏状态）会按需重建。列表最多列 20 张，超出的显示「…另有 N 张」。

| 菜单项 | 行为 |
| --- | --- |
| 便签列表（每张一行） | 显示并聚焦对应便签窗口 |
| …另有 N 张 | 超出 20 张时的占位（不可点击） |
| 新建便签 | 用 `defaultStyle` 建一张新便签（层叠偏移、夹回可见区域）并聚焦 |
| 显示全部便签 | `unminimize` + `show` 全部窗口；被隐藏的便签会重新创建窗口，并把 `visible` 置回 true |
| 隐藏全部便签 | 隐藏全部窗口，`visible` 置 false（重启后不自动出现） |
| 退出 | 先 flush 全部便签状态，再退出进程 |

菜单在**新建 / 改名 / 删除 / 折叠**后通过 `TrayIcon::set_menu` 重建；菜单对象由托盘和
`AppState` 各持一份，不会因为临时值被释放而变空。左键单击托盘图标等同「显示全部便签」。

## 已验证（本机实测，2026-11-09）

环境：rustc 1.99.1 / cargo 1.99.1、tauri 2.12.5、tauri-plugin-autostart 2.7.0、
Node 24.21.0、WebView2 157、3840×2160 @200% DPI。

- `npm run build`（`tsc --noEmit && vite build`）通过
- `cargo check` 通过且无 warning
- 端到端（WebView2 CDP 驱动真实页面）：输入文本 + 加两条待办 + 勾选 + 点图钉，
  `store.json` 里文本、TODO（含 `done`）、`alwaysOnTop: true` 全部正确落盘
- 图钉生效在系统层：窗口扩展样式出现 `WS_EX_TOPMOST`
- 用 `SetWindowPos` 挪到 300,200 / 900×700，debounce 后写入 `store.json`；
  重启进程后窗口**原样恢复到 300,200 / 900×700 且仍为置顶**
- `skipTaskbar` 由 tao 调 `ITaskbarList::DeleteTab` 实现（窗口样式仍是 `WS_EX_APPWINDOW`，
  这是正常的）；本机任务栏是自动隐藏，无法用截图佐证，只能从实现层面确认

### 0.2.0 多便签 / 外观 / 标题（release exe + WebView2 CDP 实测）

- v1 单便签 `store.json` 启动后迁移为 v2：文本、TODO、窗口几何（2105,718 / 640×840）、
  `alwaysOnTop`、`autostart` 全部保留；`store.v1.bak.json` 与原文件逐字节一致，重复重启不再迁移
- 两个便签窗口各自编辑正文互不影响（A 窗口写 A，B 窗口写 B），`notes[]` 中仅对应项变化
- 只改 A 的配色 / 主题 / 字号 / 透明度：A 的 DOM 计算样式变为 18px、`rgba(51,35,42,0.6)`、
  浅色文字，B 仍是 16px、`rgba(255,248,214,0.97)`、深色文字
- 图钉置顶态：`aria-pressed=true`、transform 为 45° 旋转矩阵、颜色加深、无背景块
- 重启后两张便签的内容 / 样式 / 置顶 / 位置全部还原，备份文件哈希不变
- `SetWindowPos` 把其中一张挪到 300,200 / 900×700，400ms 防抖后只更新该便签的 `bounds`
- 标题：点标题原地变输入框并全选，回车提交；空标题保存后回落「便签」；`Esc` 取消不写盘；
  `maxLength=60`，长标题单行省略且 `title` 属性保留全文；
  `document.title` + `set_title` 同步后，Alt+Tab / 任务管理器显示的是便签名
- 真实鼠标：点标题后 DOM 焦点落在标题输入框（进入编辑态、窗口不位移）；在标题右侧空白处拖动，
  窗口从 2105,718 移到 2205,818，400ms 后该便签的 `bounds` 同步更新
- 删除：设置面板「删除此便签」有二次确认，确认后窗口销毁、`notes[]` 对应项移除；
  删掉最后一张后进程自动退出，退出前 `notes: []` 已落盘
- Win+D（两张都未置顶，最大化记事本盖住便签）：覆盖时中心像素 `#272727` →
  第一次 Win+D 后 `#F9F4D4` / `#2B232D`（浮在桌面，桌面 `Program Manager` 在前台）→
  第二次 Win+D 后恢复 `#272727`（记事本重新盖住，`IsIconic=false`，没有被最小化）
- 便携模式：在 `target/release` 放 `portable.txt`、故意从 `%TEMP%` 作为工作目录启动，
  `store.json` 仍生成在 exe 同目录（内容从 AppData 复制，1 条便签）；便携模式里的编辑只写 exe 目录那份；
  删掉 `portable.txt` 重启回到 AppData 数据，exe 目录那份哈希不变

## 启动速度怎么测

```powershell
npm run tauri build -- --no-bundle      # 只出 exe，跳 NSIS 打包，快
pwsh -File scripts\measure-startup.ps1  # 连测 5 次（会自动杀掉残留实例）
```

脚本量两段：**进程启动 → 窗口可见**、**窗口可见 → 首帧**（窗口中心采样到便签底色即算首帧）。

注意：`npm run tauri dev` 带 Vite dev server + devtools + debug 构建，启动慢一倍以上，**不代表真实体验**，
测启动一律用 release exe。

本机实测（release，WebView2 已在内存中，3840×2160@200%）：

| 场景 | 窗口可见 | 首帧 |
| --- | --- | --- |
| 第 1 次（冷一点） | 327 ms | 459 ms |
| 第 2–5 次 | ~220 ms | ~320 ms |

开机后第一次启动会明显慢于上表（WebView2 运行时进程要冷启动），这张表是"热启动"水平。

## 抵抗 Win+D（显示桌面）

实现位置：`src-tauri/src/win_desktop_pin.rs`，setup 里一行 `win_desktop_pin::install(&window)`。
不改置顶、不改托盘、不改持久化。

### 为什么不是 tauri-plugin-wallpaper 的 pin

读过 `tauri-plugin-wallpaper` 4.1.0 的源码（`src/platform/windows/pinner.rs`）：它的 `pin`
确实**不** attach 到 WorkerW，但实现方式是给窗口挂 wndproc 子类，并在**每次** `WM_WINDOWPOSCHANGING`
里强制 `hwndInsertAfter = HWND_TOPMOST`，pin 时还直接 `SetWindowPos(HWND_TOPMOST)`。
也就是说 pin == 永久置顶，会直接破坏"便签会被前台应用正常覆盖"这条要求，还会和界面上的图钉按钮
互相打架（解除置顶后下一次位置变化又会被强制置顶回去）。所以没有采用。

### 现象

实测这台机器（Win11，3840×2160@200%）按 Win+D 的行为是：**把普通窗口最小化并停放到
`-32000,-32000`**，同时把 `Progman`（桌面窗口）抬到普通窗口之上。而便签因为
`skipTaskbar` 不在 shell 的窗口枚举里，**本来就不会被挪走/最小化**，只是被抬起的 Progman 盖住。
所以要解决的是**层级问题**，不是最小化问题——这也意味着单纯拦 `-32000` 在这里是空操作。

### 采用的机制：owner = Progman（让便签成为桌面的一部分）

把主窗口的 owner（`GWLP_HWNDPARENT`）设为桌面窗口 Progman：

- owned window 永远显示在它的 owner 之上 → 桌面被 Win+D 抬起时，便签**跟着浮在桌面上**；
- 普通应用窗口激活时照常压在便签之上 → **"会被前台覆盖"的行为完全不变**；
- 全程没有任何置顶（`WS_EX_TOPMOST`）切换，所以**不会有"跳一下"的重绘**；
- 便签仍然是普通顶层窗口，**没有** attach 到 WorkerW/壁纸层，不影响任务栏、托盘、图钉。

一个必须补的细节：新建窗口默认在最前面，而 tao 取消置顶用的是 `HWND_NOTOPMOST`，
它会把窗口顶到**普通层最前面**（表现为"取消图钉后便签反而挡住浏览器"）。
实测在 owner 绑定下 `HWND_BOTTOM` 等价于"贴着自己的 owner"，即**压在壁纸之上、其它窗口之下**，
所以启动时和取消图钉后各补一次 `SetWindowPos(HWND_BOTTOM)`。

### 为什么放弃了"临时置顶"方案

上一版用 `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 判定显示桌面，再 `SetWindowPos(HWND_TOPMOST)`
抬起来、退出时撤销。两个问题：

1. **Win+D 触发瞬间的置顶会被 shell 吞掉**：`SetWindowPos(HWND_TOPMOST)` 返回成功，但
   `WS_EX_TOPMOST` 位不变，几百毫秒后再调同样的调用才生效（单发调用实测 3 次里失败 2 次），
   只能再加"立即尝试 + 120ms × 15 轮重试"来兜。
2. 置顶/取消置顶各引发一次层级切换，视觉上便签会"跳一下"。

owner 绑定把这两个问题一起消掉了。

### 实测数据（2026-11-09，便签窗口中心像素采样）

奶油色 `#F8F1D0` 是便签本体颜色；采到别颜色说明便签被盖住。

| 场景 | 中心像素 | 结论 |
| --- | --- | --- |
| 启动后（Chrome 在前台） | `#151517` | 被覆盖（正确） |
| 最小化 Chrome，露出桌面 | `#F9F3D2` | 桌面之上可见（正确） |
| 第 1 次 Win+D | `#F9F2D2` | 浮在桌面（正确） |
| 第 2 次 Win+D | `#151517` | 应用回来、便签被覆盖（正确） |
| 第 3 / 4 次 Win+D | 交替 | 循环稳定 |
| 点图钉 → 置顶 | `#F8F1D0` | 压住 Chrome，`exstyle 0x40118` |
| 再点图钉 → 取消 | `#151517` | 回到 Chrome 后面，`exstyle 0x40110` |

### 验证步骤

```powershell
npm run tauri build -- --no-bundle
src-tauri\target\release\rust-sticky-notes.exe
```

1. 打开任意应用（浏览器等）盖住便签 → 便签应被覆盖；
2. 按 **Win+D** → 便签应仍然浮在桌面上，且进程没有被最小化（`IsIconic` 为 false）；
3. 再按一次 Win+D（或点任务栏） → 应用回来，便签重新被覆盖；
4. 点图钉置顶 → 应用盖不住它；再点一次取消置顶 → 便签回到应用后面（不是停在最前面）。

想临时关掉这个能力：注掉 `lib.rs` 里 `win_desktop_pin::install(&window);` 这一行即可，
其余功能不受影响。

## 已知限制

- Win+D 已经不被最小化了（见上节），但 **Win+M（最小化所有窗口）仍会把便签一起最小化**——
  Win+M 是系统级的"最小化"而不是"显示桌面"，owner 关系管不到；需要托盘唤回
- owner 绑在 Progman 上属于 Win32 的"拥有者"技巧，属于未文档化的行为细节：
  **explorer.exe 重启后 Progman 换了 HWND，绑定会失效**（便签退回普通窗口，Win+D 期间又会被
  桌面盖住）。当前版本没有自动重绑，重启便签即可恢复——需要的是监听 `TaskbarCreated` 广播后重新
  `install()`，留到下一阶段
- 便签现在会显示在桌面图标之上（这正是"显示桌面时可见"的由来）；如果你希望它在图标之下，就得走
  WorkerW 壁纸层方案，本阶段没做
- 没有全局快捷键，唤回只能点托盘（托盘是必须项，快捷键留到下一阶段）
- 窗口几何按**物理像素**保存：在缩放比例不同的多显示器之间挪动后，重启位置可能有偏移
- 开机自启注册的是**当前运行的可执行文件路径**：`tauri dev` 时开启自启会把 debug 版 exe 写进注册表，正式使用请先 `tauri build` 再开
- 透明窗口没有系统投影，阴影是前端 CSS 画的（`shadow: false` 避免 Windows 给无边框窗口加矩形阴影）
- 多便签 = 多个 WebView2 实例，便签开很多时内存占用线性上升（每张约几十 MB 量级）
- 深色主题 + 很低的透明度（<50%）且背后恰好是很亮的窗口时，卡片底色被稀释、浅色文字的对比度会下降；
  文字本身没有变淡，卡片边界有内描边提示，但极端场景下的可读性无法仅靠 CSS alpha 保证
- 没有 Markdown 渲染、图片、云同步
- 「新建便签」使用 `settings.defaultStyle`（默认奶油/浅色/中/97%），暂未提供修改默认值的 UI；
  已有个性化需求可以在单张便签里随时改

## 下一步

1. 全局快捷键（`tauri-plugin-global-shortcut`，例如 `Ctrl+Alt+N` 唤回）
2. 窗口贴边吸附、记住每块显示器上的位置（存 logical 坐标 + DPI）
3. 设置面板里提供"把当前外观设为新便签默认"
4. 自动保存提示细化（脏标记、退出前 flush 的可见反馈）
5. 监听 `TaskbarCreated` 后自动重绑 Progman，修复 explorer 重启导致的 Win+D 失效

## 许可证

本项目采用 MIT 许可证，详见 [LICENSE](LICENSE)。
