# 调试工具与回归脚本

## 隐藏命令行参数

几个**隐藏**命令行参数（UI 里没有入口），全部通过单实例插件转发：另起一个进程时 argv 会被送进
已运行的实例执行，所以"在正在跑的应用里执行一条命令"对用户是透明的。

```powershell
# 1) 导出窗口 + 数据状态（默认 %TEMP%\sticky-dump.json，可用 --out 改）
src-tauri\target\release\rust-sticky-notes.exe --dump [--out <路径>]

# 2) 跑 6 条不变量断言（写 %TEMP%\sticky-check.txt 和 sticky-check.json，顺带写一份 dump）
src-tauri\target\release\rust-sticky-notes.exe --check

# 3) 脚本驱动置顶（auto = z 序最靠前的那张；和 UI 图钉走同一条后端路径）
src-tauri\target\release\rust-sticky-notes.exe --set-pin <note_id|auto> <true|false>

# 4) 层位复位：把所有未置顶便签重新垫底（回归脚本的前置动作）
src-tauri\target\release\rust-sticky-notes.exe --sink-notes

# 5) 抬升全部：等价于点托盘左键，所有便签抬到前台应用之上（脚本不用去点托盘）
src-tauri\target\release\rust-sticky-notes.exe --raise-all

# 辅助（脚本内部用）
--new-note                  按 UI 路径新建一张便签
--delete-note <note_id>     删除指定便签（清理脚本自己建的临时便签）
--exit-after-dump           dump/check 完成后退出进程，退出码 = 断言结果（0 通过 / 1 有违反）
```

`--sink-notes` / `--raise-all` 是 0.4.1 新增的：前者把所有未置顶便签垫回桌面常态，
后者走与托盘左键完全相同的 `show_all_notes` 路径（**不是**裸 `raise_notes`，否则已隐藏的便签会被漏掉）。
两者都可以再跟一个 `--dump`，这样脚本等结果文件刷新就等于等命令执行完。

**没有实例在跑时**：会正常启动应用 → 等所有便签窗口创建完成 → 再执行命令，不会 dump 出半成品；
执行完不退出（除非显式 `--exit-after-dump`）。
**已经有实例在跑时**：命令由已运行实例执行，新进程立刻退出（插件统一 `exit(0)`），
所以**退出码不可信，一律以结果文件内容为准**。

> 脚本里用 `& $exe --set-pin …` 时如果没有任何实例在跑，调用会**永久阻塞**（GUI 进程起来后一直在跑）。
> 一律先 `Start-Process` 起实例，再发命令。

## `--dump` 的 JSON 契约

脚本按字段名解析，改名字要同步改 `scripts/check-zorder.ps1`：

```json
{
  "timestamp": "2026-11-11T20:09:29+08:00",
  "pid": 32332,
  "dpi_awareness": "per-monitor-v2",
  "store_path": "C:\\Users\\...\\store.json",
  "portable": false,
  "units": "physical",
  "foreground": { "hwnd": 133126, "class": "Tauri Window", "title": "便签", "is_ours": true, "z": 23 },
  "notes": [
    { "id": "7dc71de7-...", "title": "测试便签", "pinned": true, "collapsed": false,
      "visible": true, "bounds": { "x": 2062, "y": 577, "width": 640, "height": 840 } }
  ],
  "windows": [
    { "label": "note-7dc71de7-...", "note_id": "7dc71de7-...", "hwnd": 198458,
      "exstyle": "0x40118", "topmost": true, "noactivate": false, "toolwindow": false,
      "owner_hwnd": 65974, "owner_class": "Progman", "class": "Tauri Window", "title": "测试便签",
      "visible": true, "iconic": false, "focused": false,
      "rect": { "x": 2062, "y": 577, "w": 640, "h": 840 },
      "client": { "w": 640, "h": 840 }, "z": 1, "z_total": 25 }
  ],
  "violations": []
}
```

- `exstyle` 是十六进制字符串，另把 `WS_EX_TOPMOST(0x8)` / `WS_EX_NOACTIVATE(0x08000000)` /
  `WS_EX_TOOLWINDOW(0x80)` 解成 `topmost` / `noactivate` / `toolwindow`，脚本不用自己做位运算；
- 0.4.3 起 `raised` 字段已删除：`pinned` 的唯一真相就是窗口自己的 `WS_EX_TOPMOST`；
- `owner_class` 用 `GetClassNameW` 取，用来确认 owner 还是 `Progman`（owner 为 0 时 `owner_hwnd=0`、
  `owner_class=""`）；
- `notes[]` 比数据文件多一个 `visible` 字段：第 6 条不变量要靠它区分"隐藏且没有窗口"与"本该有窗口却没有"；
- `violations` 与 `--check` 的失败条目同源，空数组表示 6 条全过。

**出问题时的推荐流程**：先 `--dump`，再看 `sticky-check.txt`，把 `violations` + 对应窗口那几行
（`label / exstyle / topmost / owner_class / z / rect`）贴出来。这几个字段足够定位
"是不是丢了 TOPMOST""owner 还是不是 Progman""是不是被兄弟窗口顶上去了"三类问题，
比截图靠谱得多。

## 一键回归 `scripts/check-zorder.ps1`

```powershell
npm run tauri build -- --no-bundle
powershell -File scripts\check-zorder.ps1                  # 11 个场景 / 46 条断言 / ~10 秒
powershell -File scripts\check-zorder.ps1 -SkipRestart     # 不重启应用（更快）
powershell -File scripts\check-zorder.ps1 -Exe <别的 exe>  # 指到别的构建产物
```

覆盖的场景（11 个）：

1. 基线：便签数 ≥ 2 + 六条不变量
2. 前台覆盖（先沉底，未置顶便签不得压住前台应用）
3. `--set-pin` 切换置顶（置顶生效 / 取消置顶不沉降）
4. 取消置顶不影响兄弟（兄弟不得被连带拉起）
5. 兄弟不连坐（一钉一不钉，切 2 轮）
6. 激活 A 完全不改变 B（点击隔离）
7. 托盘抬升等价路径（`--raise-all`）
8. 关到托盘再唤回，置顶位不得丢失
9. Win+D 显示桌面再恢复，置顶位不得丢失（连做 2 轮，直接读 `WS_EX_TOPMOST`）
10. owner 链完好（只读断言 `owner_class == "Progman"`，不重启 explorer）
11. 重启一致性（关进程重启，断言置顶状态与 `store.json` 一致）

脚本做的事：确保有 ≥2 张便签（不足会自动 `--new-note` 一张，结束时删掉）→ `--sink-notes`
复位成沉底 → 把记事本拉到前台 → `--check` 断言"未置顶便签没压住前台"→ `--set-pin` 置顶一次
（断言进了置顶层）→ `--set-pin` 取消置顶一次（断言**没有沉降**：仍在记事本之上、
`topmost=false`）→ 切换 2 轮断言"未置顶那张 z 始终在前台之后"。

失败时打印 dump 里的相关原始字段并返回非 0，临时文件保留（`%TEMP%\sticky-*`），成功则删除。
它只关自己拉起来的记事本、只恢复自己改过的置顶状态，不会动用户的便签内容。

### 跑之前先关掉 PowerToys ScreenRuler

ScreenRuler 会**长期霸占前台窗口**，导致 `ForceForeground(便签)` 失败、`z` 比较失真，
表现为 3 条莫名其妙的 FAIL。这个坑踩过两次（用 `git stash` 退回旧版本能复现同样的 3 条 FAIL，
才确认不是代码问题）。

### 两个必须遵守的 PowerShell 细节

- 脚本必须用 **Windows PowerShell 5.1**（`powershell`，不是 `pwsh`）跑，并且文件本身要存成
  **UTF-8 with BOM**：5.1 在没有 BOM 时按 ANSI 解析脚本源码，中文注释会直接让它报语法错误。
  > 注意：编辑器/工具改写文件时**会剥掉 BOM**，改完脚本要重新加回去：
  > `[System.IO.File]::WriteAllText($p, $s, (New-Object System.Text.UTF8Encoding($true)))`
- 脚本内所有 `Get-Content` 也必须显式带 `-Encoding UTF8`（否则读 Rust 写出的 UTF-8 中文会乱码）。

## 层级日志

设置环境变量 `STICKY_DEBUG_ZORDER=1` 后，每次层级操作都会打印每个便签窗口的
`exstyle` / `WS_EX_TOPMOST` / owner / z 索引（默认关闭，不影响正常使用）。

出错排查时建议同时打开这个日志和 `--dump`：日志看**时序**（位是在哪一步被清掉的），
dump 看**当前状态**（z、owner、rect）。
