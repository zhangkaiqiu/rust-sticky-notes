# 窗口层级：Win+D 抵抗、置顶与契约

这是整个项目最容易踩坑的部分。改 `src-tauri/src/win_desktop_pin.rs`、图钉、折叠、新建窗口之前，
**先读完 [契约](#层级契约) 和 [历史坑位](#层级与置顶历史坑位速查)**，改完跑一遍
`powershell -File scripts\check-zorder.ps1`，全绿才算完。

## 抵抗 Win+D（显示桌面）

实现位置：`src-tauri/src/win_desktop_pin.rs`，`create_note_window` 里 `show()` 之后调一次
`win_desktop_pin::install(&window)`。不改置顶、不改托盘、不改持久化。

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

> 注意：`FindWindow("Progman", None)` 在本机返回 0，必须用 `EnumWindows` + 类名匹配来找 Progman。

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

### 手工验证步骤

```powershell
npm run tauri build -- --no-bundle
src-tauri\target\release\rust-sticky-notes.exe
```

1. 打开任意应用（浏览器等）盖住便签 → 便签应被覆盖；
2. 按 **Win+D** → 便签应仍然浮在桌面上，且进程没有被最小化（`IsIconic` 为 false）；
3. 再按一次 Win+D（或点任务栏） → 应用回来，便签重新被覆盖；
4. 点图钉置顶 → 应用盖不住它；再点一次取消置顶 → 便签**留在原处**、
   仍看得见，但不再压住其它置顶内容（不会沉降到所有窗口之下）。

想临时关掉这个能力：注掉 `lib.rs` 里 `win_desktop_pin::install(&window);` 这一行即可，
其余功能不受影响。

## 层级与置顶：历史坑位速查

这一节只留**结论**（过程叙述见 [changelog.md](changelog.md) 与 git 历史）。改
`win_desktop_pin.rs` 之前把下面四条读完，每一条都是踩过的坑：

1. **owner 同为 `Progman` 的窗口是一个 z-order 组。** 组内任何一次改 z 序的操作，
   不带 `SWP_NOOWNERZORDER` 就会牵连其它成员——清掉兄弟的 `WS_EX_TOPMOST`
   （实测 `0x40118` → `0x40110`），或把兄弟一起抬到普通层最前。
   所以**所有** `SetWindowPos` 都必须带这个标志。
2. **激活（点击）会让整组兄弟被抬到普通层最前，这条不受 `SWP_NOOWNERZORDER` 约束，也无法阻止。**
   事后补垫只会让窗口闪一下（系统先抬起并画出来，我们再压回去）。现在的做法是在
   `WM_WINDOWPOSCHANGING` 里直接给不该动的窗口加 `SWP_NOZORDER`，让它原地不动。
   实测：A 置顶、B 沉底 z=25、记事本在前台 z=4，仅激活 A 时，B 的 125 次采样全部停在 z=200，
   一次都没跑到前面。
3. **tao 的 `set_always_on_top(false)` 走 `HWND_NOTOPMOST` 且不带 `SWP_NOOWNERZORDER`，
   而且带 `SWP_ASYNCWINDOWPOS`（异步）。** 它既把窗口顶到普通层最前，又牵动整组，
   晚到的异步消息还会覆盖掉"事后再补一刀"的结果。**0.4.2 起完全不调它**，
   自己用 `place()` 做同一步。
4. **只有 `HWND_NOTOPMOST` 会清 `WS_EX_TOPMOST`**，`HWND_TOP` / `HWND_BOTTOM` 不会。
   所以"取消置顶"必须显式走 `HWND_NOTOPMOST`。
5. **`WM_WINDOWPOSCHANGING` 是唯一的拦截点**：它到达于系统真正重排并重绘**之前**。
   在这一刻把 `WINDOWPOS.flags` 加上 `SWP_NOZORDER`，窗口保持原地不动，
   既没上升也没下沉，根本没有被画到前面的机会。
6. **`SetWindowPos(HWND_NOTOPMOST, …, SWP_NOZORDER)` 不会清 `WS_EX_TOPMOST`**（实测 `0x50108` 保持不变）；
   去掉 `SWP_NOZORDER` 才会变成 `0x50100`。这条是"冻结"能生效的原理。

排查开关：环境变量 `STICKY_DEBUG_ZORDER=1` 会让每次重排打印每个窗口的
`topmost` / `exstyle` 等字段，默认关闭。

## 层级契约

这个应用的窗口层级**必须**满足下面 6 条。它们是 `--check` 的判定依据，也是任何改层级代码的
PR 合并前必须满足的条件——**改 `win_desktop_pin.rs` / 图钉 / 折叠 / 新建窗口的 PR，
请先跑一次 `powershell -File scripts\check-zorder.ps1`，全绿才算完**。

1. **置顶一致**：`store.json` 里 `pinned = true` 的便签，其窗口必须带 `WS_EX_TOPMOST`；
   `pinned = false` 的必须不带。两者任何一边单独变化都算违约。
2. **owner 正确**：每个便签窗口的 owner（`GWLP_HWNDPARENT`）类名必须是 `Progman`。
   这是 Win+D 抗性的前提，owner 一旦丢掉，按 Win+D 便签就会被桌面盖住。
3. **不越界**：未置顶便签窗口的 z 序必须**比当前前台窗口更靠后**（z 值更大），
   也就是不能被后台便签压住前台应用。前台窗口本身是便签时这条不适用（跳过）。
4. **置顶在最前**：所有置顶窗口的 z 都小于所有未置顶窗口的 z——置顶的在最上层，
   没置顶的一律垫在下面，不存在"半高不低"的夹心状态。
5. **几何一致**：展开态便签窗口的 `rect` 宽高必须等于 `bounds` 宽高（容差 2px 物理像素）；
   折叠态跳过（`bounds` 存的是展开态几何，窗口却是 48 逻辑像素高，本来就不相等）。
6. **窗口与数据对得上**：`store.json` 的 `notes[]` 与运行中的便签窗口 id 集合必须一一对应，
   每个 id 恰好一个窗口、不多不少。`visible=false`（启动时被隐藏、没建窗口）的便签允许没有窗口。

判定口径（脚本依赖）：

- `z` 是 `EnumWindows` 枚举到的**全体可见顶层窗口**序号，`0 = 最前`；
  不在该枚举里的窗口（隐藏 / 最小化到托盘）写 `null`；
- 所有矩形都是**物理像素**（`--dump` 顶层有 `"units": "physical"`）；
- `--check` 的 `PASS 6/6` 里，SKIP 也计入通过（例如前台恰好是便签自己），
  但"场景断言"（脚本里把外部应用拉到前台的那些）要求对应条目必须是 `pass`，不允许靠 skip 蒙混。

## 一个窗口只操作自己

**铁律：对 A 的任何操作都不得改变 B。**

0.4.1 之前是一堆"每条路径补一刀"的碎片补丁（`park_behind` / `park_siblings_behind` /
`repair_pinned_topmost` 各在不同路径上打点）。0.4.2 改成"状态 + 一次全量 `reconcile()`"：
每张便签在内存里记 `{pinned, raised}`，任何入口都只做"改状态 → 全量重排"。

它算得没错，但**模型本身是错的**：每次点 A 都会给所有窗口重写一遍 z 序，于是只要某张兄弟
残留了 `raised=true`，点 A 一下就被顶到浏览器前面。而 `raised` 会泄漏——除了 CLI 的
`--sink-notes`，没有任何路径把它清回 false（取消置顶、被点中都会写 `true`）。用户报的
「点 A 的文本区，B 就窜到前面」就是它。

0.4.3 把状态表和全量重排**整段删除**（706 → 268 行）：

- **没有内存状态**：`pinned` 的唯一真相是窗口自己的 `WS_EX_TOPMOST`，每次操作前现读；
- **没有全量重排**：`set_user_pinned` 只对自己调一次 `SetWindowPos`；
- `install()` 只绑 owner，**不动 z 序**（新窗口本来就该在最前）；
- 只剩一处"碰别人"，即系统强加的那一笔：激活 owner 组会把整组抬起来，事后由
  `WM_WINDOWPOSCHANGING` 钩子直接给不该动的窗口加 `SWP_NOZORDER`，让它原地不动。

对外入口一共 5 个，语义都不再依赖任何状态位：

| 入口 | 时机 | 动谁 |
| --- | --- | --- |
| `install()` | 窗口 `show()` 后、`set_focus()` 前 | 只绑 owner，不动 z 序 |
| `set_user_pinned()` | UI 图钉 / 设置面板 / `--set-pin` | **只有这一张** |
| `WM_WINDOWPOSCHANGING` 钩子（`note_wndproc`） | 系统每次想改这张便签的 z 序 | 该窗口自己（原地不动） |
| `raise_notes()` | 托盘左键 / `--raise-all` | 全部（用户显式要求的全局动作） |
| `sink_all_notes()` | `--sink-notes`（回归复位） | 全部未置顶的 |

`raise_notes()` 会开一段 1.2 秒的静默期（`QUIET_UNTIL_MS`），否则紧随其后的
`Focused(true)` 会把除焦点窗口外的便签又踩回去，用户看到"点托盘只上来一张"。

## 当前的冻结规则（0.4.6）

`note_wndproc` 收到 `WM_WINDOWPOSCHANGING` 时的判断，是整个机制的核心：

```rust
let self_initiated = SELF_ZORDER.load(Ordering::SeqCst);
let after = pos_ref.hwndInsertAfter;
let protect_pin = !self_initiated && has_topmost(hwnd) && after != HWND_TOPMOST;
let freeze = if self_initiated { false } else if protect_pin { true } else { should_freeze(hwnd) };
// freeze => (*pos).flags |= SWP_NOZORDER
```

翻译成人话：

> **只要这次 z 序改动不是我们自己发起、且插入目标不是 `HWND_TOPMOST`，就原地冻结。**

置顶窗口本来就该待在最顶层，被插到任何别处都是错的；原地不动没有视觉副作用。
自己发起的改动（`SELF_ZORDER` 为真）照常放行，所以 `set_user_pinned(false)` 仍然生效。

`should_freeze()` 只服务**未置顶**窗口，返回 false 的情形：自己发起的、静默期内、
当前是前台窗口、鼠标正压在窗口上、刚收到 `WM_MOUSEACTIVATE` / `WM_LBUTTONDOWN`
（激活发生在 z 序变化之前，靠这个提前放行）。

另外两个必须保留的保护：

- **`WM_STYLECHANGING` 锁**：`wparam == GWL_EXSTYLE` 且非自己发起时，把 `styleNew` 的
  `WS_EX_TOPMOST` 位**锁成 `styleOld` 的值**。这是为了挡 tao 的破坏——
  `WindowFlags::apply_diff()` 收尾无条件 `SetWindowLongW(GWL_EXSTYLE, ...)`，
  按 tao **内部**记录的旧 flags 重拼整条 exstyle。
  > `GWL_EXSTYLE` 是 `WINDOW_LONG_PTR_INDEX`，比较时要写 `GWL_EXSTYLE.0`，不能直接比裸常量。
- **`TaskbarCreated` 自动重绑**：`explorer.exe` 重启后 Progman 换了新 HWND，owner 绑定失效
  （置顶位还在，但 Win+D 时被桌面盖住，表现极像"置顶失效"）。窗口过程里处理
  `TaskbarCreated` 广播 → 重新 `bind_owner(hwnd)`。实测杀 explorer 后 owner 从
  `132258 → 331112` 自动改绑，置顶位保持 `0x40118`。

## 为什么彻底不用 tao 的 `set_always_on_top`

它走 `SetWindowPos(HWND_NOTOPMOST, …, SWP_ASYNCWINDOWPOS|SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`，
**不带** `SWP_NOOWNERZORDER` 且是**异步**的。实测（`--dump` 同一次枚举取 z）：

- 只发 `NOTOPMOST`（不带 NOOWNERZORDER）：A 从 `ex=0x40118` 变 `0x40110`，**B 从 z=52 跳到 z=5**，
  越过了记事本（z=6）——这就是用户看到的"两张便签联动"；
- 同样操作带 `SWP_NOOWNERZORDER`：A 从 z=2 到 z=4，**B 停在 z=52 不动**。

异步那条更糟：`NOTOPMOST` + 立刻 `HWND_BOTTOM` 垫兄弟时，异步消息晚到会把垫底结果覆盖掉
（实测 B 从 z=52 变成 z=5），所以任何"先 tao 再补一刀"的方案都不可靠。

## 试过但无效的路（不要再走）

- **事后补垫**：系统先把组抬起并画出来，我们再 10~150ms 压回去，视觉上就是闪一下。
- **给每条便签各挂一个屏幕外 1×1 影子窗口做 owner**：影子自身又同属 Progman，
  组只是往下一层挪，激活 A 照样把 B 的整条链抬起来（实测 B 仍然 27 → 5）。
- **`HWND_TOPMOST` 轮询 / `SetWinEventHook` 临时置顶**：Win+D 触发瞬间会被 shell 吞掉，
  且每次切换都有视觉跳变。见上文「为什么放弃了临时置顶方案」。
- **持久化 `raised` 之类的状态位**：会泄漏，且"操作 A 影响 B"必然复发。
