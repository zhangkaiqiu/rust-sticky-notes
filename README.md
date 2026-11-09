# rust-sticky-notes

Windows 桌面便签（Tauri 2 + Rust + React + TypeScript + Vite + Tailwind CSS）。

无边框、透明背景、不在任务栏显示图标的普通窗口，默认不置顶（会被前台应用正常覆盖），
通过系统托盘唤回，图钉按钮切换 alwaysOnTop，文本 / TODO / 窗口位置尺寸持久化到
`%APPDATA%\com.ruststicky.notes\store.json`。

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
├── scripts/gen_icon.py         # 用 Pillow 生成 1024px 源图标（一次性脚本）
├── src/                        # 前端
│   ├── main.tsx
│   ├── App.tsx                 # 状态编排 + 防抖保存
│   ├── api.ts                  # invoke 封装
│   ├── types.ts                # 与 Rust store 结构对应
│   ├── styles.css              # Tailwind + 滚动条等基础样式
│   └── components/
│       ├── TopBar.tsx          # 拖动区 + 图钉 + 设置
│       ├── TodoList.tsx        # 待办增删改勾选
│       ├── SettingsPanel.tsx   # 置顶/自启/隐藏/退出/数据路径
│       └── icons.tsx           # 内联 SVG，无图标库依赖
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json         # 窗口：无边框/透明/跳过任务栏/320x420
    ├── capabilities/default.json
    ├── icons/                  # 由 `tauri icon` 生成
    └── src/
        ├── main.rs
        ├── lib.rs              # 命令 + 托盘 + 窗口几何持久化
        ├── store.rs            # JSON 读写（原子替换）
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
npm run tauri build
```

产物：

- 可执行文件：`src-tauri/target/release/rust-sticky-notes.exe`
- 安装包：`src-tauri/target/release/bundle/nsis/Rust Sticky Notes_0.1.0_x64-setup.exe`

NSIS 安装模式为 `currentUser`（不需要管理员权限）。

## 已实现

- 窗口：`decorations: false` / `transparent: true` / `skipTaskbar: true` / `alwaysOnTop: false`，初始 320×420，可拖动（顶部 `data-tauri-drag-region`）、可拉伸
- 便签文本：textarea，停止输入 450ms 后 `invoke("save_note")` 落盘
- TODO：添加（回车）、勾选完成、行内改文字、删除、批量清除已完成，与文本一起持久化
- 图钉：切换 `set_always_on_top`，按钮高亮表示当前状态，状态存 `settings.alwaysOnTop`，下次启动恢复
- 托盘：左键唤回（`unminimize` → `show` → `set_focus`），右键菜单「显示便签 / 隐藏便签 / 退出」
- 窗口位置与大小：监听 `Moved` / `Resized`，400ms 静默后写盘；启动时恢复，若上次所在显示器已不存在则回主屏居中
- 开机自启：设置面板开关直连 `tauri-plugin-autostart`，每次启动以系统真实状态为准回写 JSON
- 关窗即隐藏（`CloseRequested` 里 `prevent_close` + `hide`），进程常驻托盘
- **抵抗 Win+D**：`owner = Progman` 桌面绑定，显示桌面后便签浮在桌面上；不改置顶、不改"被前台应用覆盖"、
  层级全程不切换所以也不"跳"（详见下文「抵抗 Win+D」一节）

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
- 单便签、纯文本：不支持多便签、Markdown 渲染、图片、云同步

## 下一步

1. 全局快捷键（`tauri-plugin-global-shortcut`，例如 `Ctrl+Alt+N` 唤回）
2. 多便签：`store.json` 改成 `notes: [{id, ...}]` + 每个便签一个窗口
3. 便签配色 / 字号选择，深浅主题
4. 窗口贴边吸附、记住每块显示器上的位置（存 logical 坐标 + DPI）
5. 自动保存提示细化（脏标记、退出前 flush）
