# 版本变更与验证记录

每个版本的验证都是**脚本自证**，不是"看了一眼截图觉得对"。完整的层级机制说明见
[window-layering.md](window-layering.md)，回归脚本用法见 [diagnostics.md](diagnostics.md)。

## 0.1.x 基线（本机实测，2026-11-09）

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

## 0.2.0 多便签 / 外观 / 标题（release exe + WebView2 CDP 实测）

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

## 0.3.0 快速冒烟（release exe + 临时 portable 目录，200% DPI）

- **折叠**：点标题栏箭头，窗口物理高度 840 → 96（= 48 逻辑像素），`store.json` 的
  `collapsed: true`、`bounds.height` 仍为 840；再点恢复 840
- **右键菜单**：正文右键弹出「剪切 / 复制 / 粘贴 / 全选 / 新建便签 / 折叠成条 / 设置… / 删除便签」，
  无选区时剪切 / 复制置灰；点「粘贴」把系统剪贴板内容插到光标处（`execCommand('insertText')`）
- **Ctrl + 滚轮**：上滚 `fontSize` 由 `md` → `lg`（到顶停住），300ms 防抖后落盘，卡片右下角浮出
  「字号：中」轻提示；下滚回到 `md`
- **新建便签**：右键菜单「新建便签」后 `notes` 变为 2 条，两个窗口各自独立
- **单实例**：再启动一份 exe，新进程立即退出，仍只有 1 个实例、便签数不变
- **每日备份**：当天首次写盘生成 `store.20261109.json`；预置 9 份旧备份后再落盘，只保留最近 7 份
  （最旧的 3 份被删除）

## 0.4.0 诊断基建（`--dump` / `--check` / `scripts/check-zorder.ps1`，2026-11-11）

这一节不是"看了一眼截图觉得对"，而是脚本自证：

- `src-tauri\target\release\rust-sticky-notes.exe --check` → `PASS 6/6`，六条不变量逐条见
  `%TEMP%\sticky-check.txt`（前台是便签自己的窗口时第 3 条会 SKIP，脚本里会先把记事本拉到前台再断言）
- `pwsh -File scripts\check-zorder.ps1` → **18/18 通过**，耗时约 **4.7s**（含一次关进程重启）
- 顺带抓到并修掉第三个层级 bug（取消置顶把兄弟窗口的 `WS_EX_TOPMOST` 一起清掉）
- 用外部脚本恶意清掉某个窗口的 `WS_EX_TOPMOST` 后重跑，第 1 条不变量立刻 FAIL，
  失败上下文会打印原始字段（`exstyle` / `owner_class` / `z` / `rect`），验证了"坏状态一定能被判出来"

## 0.4.1 层位意图（Sunk / Raised，2026-11-11；0.4.3 已移除）

- 取消置顶不再沉降：`--set-pin <id> false` 之后 dump 里该窗口 `topmost=false`、`raised=true`，
  且 z **仍小于**记事本 z（以前会被 `HWND_BOTTOM` 踩到所有窗口之下）
- 托盘左键（`--raise-all` 等价路径）：全部便签 `raised=true`，所有便签 z 都小于记事本 z；
  1.5s 后再 dump 仍然成立（不会被随后的 `Focused(true)` 踩回去）
- `powershell -File scripts\check-zorder.ps1` → **23/23 通过**，连续跑 3 次结果一致
  （单次结果不可信：人可能在旁边动鼠标）

## 0.4.2 层级重写 + 菜单职责剥离（2026-11-11）

- `win_desktop_pin.rs` 从 706 行（663 行非空）重写为约 430 行：删除 4 段历史 bug 过程注释、
  `clear_topmost` 死代码、`debug_dump`/`collect_zorder`（诊断层已有一份）、
  两个 `Mutex<Option<HashMap>>` 状态表合并成一张有序 `Vec`
- 修掉「**取消置顶把兄弟便签一起拉上来**」：`--set-pin <A> false` 之后立刻 dump，
  A `topmost=false`/`raised=true`/z 前台之前，B `raised=false`/z 前台之后（不再被连带）
- `powershell -File scripts\check-zorder.ps1` → **28/28 通过**，连跑 3 次结果完全一致
- 菜单职责剥离：「开机自启」「退出应用」从便签设置面板移到托盘菜单（自启带勾选态），
  删除随之无用的 `set_autostart` / `quit_app` 命令与 `PowerIcon`

## 0.4.3 一个窗口只操作自己，且不闪（2026-11-11）

**用户反馈的体验问题**：「窗口闪一下，又消失」。0.4.2 的修法是激活后把兄弟便签"压回底部"——
但系统是先把整组抬到最前、画出来，我们 10~150ms 后再压回去，视觉上就是闪一下。

试过但**无效**的两条路：

- **事后补垫**（0.4.2 末）：只是把抖动缩短，B 仍然先被画到前面；
- **给每条便签各挂一个屏幕外 1×1 影子窗口做 owner**：影子自身又同属 Progman，
  组只是往下一层挪，激活 A 照样把 B 的整条链抬起来（实测 B 仍然 27 → 5）。

最终做法是**在系统改 z 序的那一刻拦掉**：给便签窗口装窗口过程钩子，拦
`WM_WINDOWPOSCHANGING`（它到达于系统真正重排并重绘**之前**）。当这次 z 序改动不是这张
便签自己的交互引起时，直接给 `WINDOWPOS.flags` 加上 `SWP_NOZORDER`——窗口保持**原地不动**，
既不上升也不下沉，根本没有被画到前面的机会。

关键区别：是"原地不动"而不是"压到底部"，所以用户自己抬起来过的便签不会被 A 顺带下沉。

实测：真实鼠标点击 A，以每次采样重新 `EnumWindows` 的方式连续采 125 次，B 的 z 一直是 200，
**跑到浏览器之前的次数 = 0**（0.4.2 末版是每次点击都有 4~5 个采样落在前面，即用户看到的闪）。

用户报障原话：「A 置顶、B 不置顶，最顶层是浏览器；我只要一点 B（不是点图钉，是点任何位置），
B 就会拉到前台」——以及更根本的要求：**对 A 的所有操作都不该改变 B 的任何状态**。

- 根因是 0.4.2 的全量 `reconcile()` + 泄漏的 `raised` 状态位：每次点 A 都重写所有窗口 z 序，
  B 只要残留 `raised=true`（取消置顶/被点中都会写进去，且没有路径清回 false）就被顶上来。
  实测把 B 的 `raised` 强制置 false 后点 A，B 纹丝不动 → 确认是我们的代码在拉它
- `win_desktop_pin.rs` 从 434 行减到 268 行：删掉状态表、`Mutex`、`reconcile()`、
  三段式排序、`note_activated()`；`install()` 只绑 owner 不动 z 序；
  `set_user_pinned()` 只对自己调一次 `SetWindowPos`
- 但"事后补垫"仍会让窗口闪一下（系统先抬起并画出来，我们再压回去），见上
- `--dump` 删除 `raised` 字段，不变量 3「不越界」不再有 raised 例外
- 真实鼠标点击 A 后以 15ms 采样 20 次，B 的 z 序号一次都没变（0/20）
- `powershell -File scripts\check-zorder.ps1` → **31/31 通过**（新增「点击隔离」场景），
  连跑 3 次结果完全一致

## 0.4.4 置顶位被 tao 抹掉 + 托盘唤不回（2026-11-11）

用户点了「诊断：导出窗口状态」，导出的 JSON 自己报了警：

```
A: pinned=true(store)  topmost=false  exstyle=0x40110  z=25/27
B: pinned=false        topmost=false  exstyle=0x40110  z=6/27
violations:
  · 置顶一致：A 数据 pinned=true 但窗口 topmost=false
  · 置顶在最前：置顶窗口 A z=25 落在未置顶窗口 B z=6 之后
```

**这不是层序问题，是 `WS_EX_TOPMOST` 位整个没了。** 两个独立 bug 叠在一起：

### bug 1：tao 每次重算 flags 都会把置顶位抹掉

`tao` 的 `WindowFlags::apply_diff()` 收尾时无条件执行：

```rust
let (_, style_ex) = new.to_window_styles();   // 按 tao **内部**记录的 flags 重拼
SetWindowLongW(hwnd, GWL_EXSTYLE, style_ex);  // 直接覆盖整条 exstyle
```

而我们切换置顶时刻意绕开 tao（它的 `set_always_on_top` 带 `SWP_ASYNCWINDOWPOS`
且不带 `SWP_NOOWNERZORDER`，见 0.4.2），直接调 `SetWindowPos`。于是 **tao 内部
`ALWAYS_ON_TOP` 位永远停在创建时的值**。之后任何一次 tao flags 重算——关到托盘再唤回
（`set_visible`）、最小化等——都会拿这份旧模型覆盖整条 exstyle，把置顶位悄悄抹掉。

没有任何路径会把它恢复：`apply_pinned` 看到"值没变"直接返回，`raise_notes` 看到
`has_topmost=false` 就走 `HWND_TOP`。于是便签沉在浏览器后面，而 store 里仍写 `pinned=true`。

抓证据时踩的坑：

- 一开始给 `WM_WINDOWPOSCHANGING` 加 `trace-z`，**一条都没打**——说明不是
  `SetWindowPos` 清掉的（`trace-z` 只记 `SWP_NOZORDER` 之外的调用）；
- 再加 `WM_STYLECHANGING` 追踪，一次命中：`[trace-topmost] B old=0x40118 new=0x40110`。

修法：在窗口过程的 `WM_STYLECHANGING` 里把 `styleNew` 的该位**锁成 `styleOld` 的值**，
等于"除我们自己以外谁也改不动置顶位"（`place()` 期间靠 `SELF_ZORDER` 放行）。

### bug 2：`--raise-all` 唤不回已隐藏的便签

原来 `run_cli_after_note` 里 `--raise-all` 只调 `win_desktop_pin::raise_notes()`，
而它只处理"已经存在且可见"的窗口。收进托盘的便签被 `hide()` 过，直接被漏掉——
注释说它"等价于点托盘左键"，实际并不等价（托盘走的是 `show_all_notes`，里面有
`ensure_window_visible`）。

修法：`--raise-all` 改调 `show_all_notes(app)`，与托盘完全同一条路径。

### 回归

新增场景「关到托盘再唤回，置顶位不得丢失」（发 `WM_CLOSE` → 等
`IsWindowVisible=false` → `--raise-all` → 直接读 `WS_EX_TOPMOST` 断言）。
这条路径以前**完全没被覆盖**，正是它让 bug 一直逃过 31/31 的全绿。

`powershell -File scripts\check-zorder.ps1` → **38/38 通过**（9 个场景），连跑 3 次一致。

## 0.4.5 Win+D 恢复窗口会清掉置顶位（2026-11-12）

用户报障：「我在桌面按 Ctrl + D，置顶就失效了」，并导出 `%TEMP%\sticky-dump.json`：
A 的 `pinned=true` 但 `exstyle=0x40110`（无 `WS_EX_TOPMOST`）。

**实测定位到不是 Ctrl+D，是 Win+D 第二次（从桌面恢复窗口）。** 按键序列逐帧采样：

```
0 已置顶              A topmost=True  ex=0x40118
1 Win+D（显示桌面）    A topmost=True  ex=0x40118   ← 没问题
2 Ctrl+D（桌面焦点）   A topmost=True  ex=0x40118   ← 没问题
3 Win+D（恢复窗口）    A topmost=False ex=0x40110   ← 位在这里被清掉 ★
5 Ctrl+D 再来一次      A topmost=False              ← 已丢失，看起来像 Ctrl+D 干的
```

临时给窗口过程加 trace，抓到恢复瞬间的消息：

```
[tz] CHANGING topmost=true  after=527078 flags=0x13 self=false
[tz] CHANGED  topmost=false
```

`after` 是**具体 HWND**，不是 `HWND_NOTOPMOST`。shell 恢复窗口时把便签插回普通层，
这个 `WM_WINDOWPOSCHANGING` 会连带清掉 `WS_EX_TOPMOST`，而且**不经过
`WM_STYLECHANGING`** —— 所以 0.4.4 在样式变更里加的那把锁完全挡不住。

而 `should_freeze()` 里有一条 `if has_topmost(hwnd) { return false; }`
（"置顶窗口本来就该在最前，一律放行"），正好让这条消息长驱直入。

修法：去掉这条放行，让**置顶窗口也接受 `SWP_NOZORDER` 冻结**。冻结只是让它原地不动，
置顶窗口本来就在最前，没有任何视觉副作用。另加一条 `HWND_NOTOPMOST` 精确拦截作双保险。

回归新增场景「Win+D 显示桌面再恢复」——连做 2 轮，每轮直接读窗口的
`WS_EX_TOPMOST` 位，不通过任何 JSON 判断。这是第二次有 bug 从「桌面/托盘往返」
这类路径漏进来，现在两条都固化了。

`powershell -File scripts\check-zorder.ps1` → **45/45 通过**（10 个场景），连跑 3 次一致。

## 0.4.6 置顶位又被抹掉：认不出「插到具体 HWND 之后」（2026-11-12）

用户跑满 6.5 小时后又报同一个症状，导出的 dump 依旧是 `pinned=true` 但 `exstyle=0x40110`。
这次窗口还被拖动过（`bounds` 变了），于是先把「拖动 / 折叠 / 悬停 / 点按 / Alt+Tab /
任务栏显示桌面 / Win+M / 虚拟桌面」逐条用真实鼠标键盘重放了一遍——**全部没复现**。

改用「模拟外部攻击」才定位到：直接对一个已置顶的便签调
`SetWindowPos(HWND_NOTOPMOST)`，一次就把位清掉了。加 trace 后看到真相：

```
[wz] CHANGING top=true  after=919580 flags=0x13 self=false
[wz] -> freeze=false                  ← 没拦住
[wz] CHANGED  top=false               ← 位被清掉
```

0.4.5 只精确匹配 `hwndInsertAfter == HWND_NOTOPMOST`（-2），而实际消息里的
`after` 是**具体 HWND**——「插到某个窗口之后」同样会让便签掉出置顶层、连带清掉
`WS_EX_TOPMOST`。加上 `should_freeze` 里还有几条放行口子（前台窗口 / 鼠标正压着 /
刚按过），一旦命中就漏。这正好解释「偶发、要跑几小时才出现一次」：取决于那一刻
鼠标和焦点在哪。

修法：对**置顶窗口**不再逐条判断放行条件，直接定一条铁律——

> 只要这次 z 序改动不是我们自己发起、且插入目标不是 `HWND_TOPMOST`，就加
> `SWP_NOZORDER` 原地冻结。

置顶窗口本来就该待在最顶层，被插到任何别处都是错的；原地不动没有视觉副作用。
我们自己的 `place()` 期间 `SELF_ZORDER` 为真，所以 `set_user_pinned(false)` 照常生效。

**同时修掉一个长期潜伏的缺陷**：`explorer.exe` 重启后 Progman 换了新句柄，owner 绑定
失效——置顶位还在，但抗 Win+D 失效（Win+D 时被桌面盖住），表现极像「置顶失效」。
现在在窗口过程里处理 `TaskbarCreated` 广播自动重绑。实测：杀掉 explorer 后 owner
从旧句柄自动改绑到新 Progman（`132258 → 331112`），置顶位保持 `0x40118`。

攻击矩阵（全部被挡住，6/6）：`SetWindowLongPtr` 清位、`HWND_NOTOPMOST`、
`HWND_BOTTOM`、`HWND_TOP`、伪造 `WM_STYLECHANGED`、`TOPMOST→NOTOPMOST` 组合。
反向也验过：取消置顶 / 重新置顶仍然正常生效，没有把功能锁死。

计算机操作验证（真实鼠标键盘，13 组）：Win+D 多轮、任务栏「显示桌面」、Win+M、
Alt+Tab、鼠标悬停时 Win+D、点击后立即 Win+D、拖动窗口后 Win+D、UI 点图钉双向、
折叠/展开、折叠态 Win+D、3 窗口叠加、explorer 重启。全程 300 秒后台监控
`WS_EX_TOPMOST` 位，除刻意切换置顶外**零意外翻转**。

回归新增场景「owner 链完好」（只读断言，不重启 explorer），
`powershell -File scripts\check-zorder.ps1` → **46/46 通过**（11 个场景），连跑 3 次一致。
