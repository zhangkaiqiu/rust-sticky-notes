# rust-sticky-notes

Windows 桌面便签（Tauri 2 + Rust + React + TypeScript + Vite + Tailwind CSS）。

**每张便签一个独立窗口**，支持 6 套配色、3 档字号、深浅主题与可调透明度；基于
`owner = Progman` 的桌面绑定实现 Win+D 抵抗，同时保留"平时会被前台应用正常覆盖"的层级。

A 置顶时它压住浏览器，B 不置顶时它沉在后面——**对 A 的任何操作都不会改变 B 的状态**。

## 特性

- **多便签**：每张独立窗口，位置 / 大小 / 配色 / 字号 / 主题 / 透明度 / 置顶全部按便签保存
- **抗 Win+D**：按 Win+D 便签仍浮在桌面上，但平时会被前台应用正常覆盖（不靠全局置顶实现）
- **图钉置顶**：只影响这一张；取消置顶**不沉降**、也不连累兄弟便签
- **折叠成条**：一键缩成 48 逻辑像素的标题栏
- **TODO 列表**：与正文一起持久化
- **右键菜单**：剪切 / 复制 / 粘贴 / 全选 + 便签操作，粘贴保留撤销栈
- **Ctrl + 滚轮**调字号
- **托盘常驻**：便签列表、新建 / 全部显示 / 全部隐藏、开机自启、退出
- **每日自动备份**：保留最近 7 份
- **便携模式**：exe 旁边放个空 `portable.txt`，数据就跟着程序走

## 快速开始

### 直接用

从 [Releases](https://github.com/zhangkaiqiu/rust-sticky-notes/releases) 下载，二选一：

| 文件 | 说明 |
| --- | --- |
| `rust-sticky-notes_0.4.6_x64-portable.zip` | 免安装：解压即用，数据写在 exe 同目录 |
| `Rust_Sticky_Notes_0.4.6_x64-setup.exe` | NSIS 安装包（装到当前用户，不需要管理员） |

单文件运行，**不需要 Node.js，也不需要 Rust**。唯一依赖是系统的 WebView2
（Windows 11 自带；Windows 10 若没有，安装包会自动补）。

### 从源码构建

首次需要 Node 18+、Rust 1.77+（stable-msvc）、MSVC Build Tools、WebView2 Runtime。

```powershell
npm install
npm run tauri dev                    # 开发模式（Vite HMR + Rust 热重启）
npm run tauri build -- --no-bundle   # 出 release exe
```

详细步骤、NSIS 打包、启动速度测量见 [docs/architecture.md](docs/architecture.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | 技术栈、目录结构、运行、打包、启动速度 |
| [docs/features.md](docs/features.md) | 完整功能清单、右键菜单、托盘菜单、菜单职责边界 |
| [docs/data-and-storage.md](docs/data-and-storage.md) | `store.json` 格式、v1 迁移、每日备份、便携模式 |
| [docs/window-layering.md](docs/window-layering.md) | **Win+D 抵抗原理、层级契约、置顶冻结规则、历史坑位** |
| [docs/diagnostics.md](docs/diagnostics.md) | 隐藏命令行参数、`--dump` JSON 契约、回归脚本 |
| [docs/changelog.md](docs/changelog.md) | 各版本变更与验证记录 |

改窗口层级相关代码（`win_desktop_pin.rs` / 图钉 / 折叠 / 新建窗口）之前，
请先读 [docs/window-layering.md](docs/window-layering.md) 的「层级契约」一节，
改完跑一次回归：

```powershell
powershell -File scripts\check-zorder.ps1     # 11 个场景 / 46 条断言
```

## 已知限制

- **Win+M（最小化所有窗口）仍会把便签一起最小化**——Win+M 是系统级"最小化"而不是"显示桌面"，
  owner 关系管不到；需要托盘唤回
- 便签同属一个 owner 组，"激活连带"在 Windows 侧无法阻止，只能由 `WM_WINDOWPOSCHANGING`
  钩子拦成 `SWP_NOZORDER`（窗口原地不动，实测窗口期不可见）
- 便签会显示在桌面图标之上（这正是"显示桌面时可见"的由来）；若希望在图标之下，
  需走 WorkerW 壁纸层方案，当前未实现
- 窗口几何按**物理像素**保存：在缩放比例不同的多显示器之间挪动后，重启位置可能有偏移
- 开机自启注册的是**当前运行的可执行文件路径**：`tauri dev` 时开启自启会把 debug 版 exe
  写进注册表，正式使用请先 `tauri build` 再开
- 单实例锁按应用 identifier 判定（不是按 exe 路径），**便携模式的两份拷贝同机运行**时
  第二份会被当成"已有实例"退出
- 透明窗口没有系统投影，阴影是前端 CSS 画的
- 多便签 = 多个 WebView2 实例，便签开很多时内存线性上升（每张约几十 MB）
- 深色主题 + 很低透明度（<50%）且背后恰好是很亮的窗口时，浅色文字对比度会下降
- 「新建便签」使用 `settings.defaultStyle`（默认奶油 / 浅色 / 中 / 97%），暂未提供修改默认值的
  UI；已有个性化需求可以在单张便签里随时改
- exe 未做代码签名，首次运行可能提示 SmartScreen，点「更多信息 → 仍要运行」即可
- 没有全局快捷键、Markdown 渲染、图片、云同步

## 下一步

1. 全局快捷键（`tauri-plugin-global-shortcut`，例如 `Ctrl+Alt+N` 唤回）
2. TODO 提醒（到点提醒 / 托盘气泡）
3. 全文搜索（跨便签搜标题与正文）
4. Markdown 渲染
5. 窗口贴边吸附、按显示器记住位置（存 logical 坐标 + DPI）
6. 自动更新（`tauri-plugin-updater`）
7. 设置面板里提供"把当前外观设为新便签默认"

## 许可证

本项目采用 MIT 许可证，详见 [LICENSE](LICENSE)。
