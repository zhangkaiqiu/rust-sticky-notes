# 架构与构建

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
├── docs/                       # 本目录：分层后的详细文档
├── scripts/gen_icon.py         # 用 Pillow 生成 1024px 源图标（一次性脚本）
├── scripts/measure-startup.ps1 # 启动耗时测量
├── scripts/check-zorder.ps1    # **窗口层级回归断言**（--dump / --check 驱动，见 diagnostics.md）
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
│       ├── SettingsPanel.tsx   # 配色/字号/主题/透明度 + 置顶/隐藏/删除（只放本便签的项）
│       └── icons.tsx           # 内联 SVG，无图标库依赖
└── src-tauri/
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json         # 不在配置里预建窗口，全部由 Rust 按便签创建
    ├── capabilities/default.json
    ├── icons/                  # 由 `tauri icon` 生成
    └── src/
        ├── main.rs
        ├── lib.rs              # 命令 + 每便签窗口 + 托盘 + 几何持久化 + 调试命令行分发
        ├── store.rs            # store.json v2 读写（v1 迁移 + 原子替换）
        ├── cli.rs              # --dump / --check / --set-pin 等隐藏调试参数解析
        ├── diagnostics.rs      # 窗口/数据快照（JSON）+ 6 条层级不变量断言
        └── win_desktop_pin.rs  # 抵抗 Win+D（owner = Progman）+ 图钉，每个窗口只操作自己
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
- 安装包（去掉 `--no-bundle` 时）：`src-tauri/target/release/bundle/nsis/Rust_Sticky_Notes_0.4.6_x64-setup.exe`

**必须走 Tauri CLI**：直接 `cargo build --release` 不会带上 `custom-protocol` 特性，
打出来的 exe 会去连 `localhost:1420`，显示"无法访问此页面"。

NSIS 安装模式为 `currentUser`（不需要管理员权限）。

## 启动速度怎么测

```powershell
npm run tauri build -- --no-bundle      # 只出 exe，跳 NSIS 打包，快
pwsh -File scripts/measure-startup.ps1  # 连测 5 次（会自动杀掉残留实例）
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
