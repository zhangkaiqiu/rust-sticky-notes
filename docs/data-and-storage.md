# 数据文件、备份与便携模式

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
      "collapsed": false,
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
- `collapsed: true` 只说明"窗口现在收成条"：`bounds` 里存的仍是**展开态**宽高，启动时按 `collapsed`
  把窗口压到 48 逻辑像素，展开时再按 `bounds.height` 恢复

**从 v1（单便签）迁移**：

- 启动时检测 JSON 里有没有 `notes` 数组：没有就按旧格式 `{ text, todos, settings, window[, style] }` 解析
- 迁移前先把原文件备份成同目录的 `store.v1.bak.json`（备份已存在则不覆盖，便于回溯）
- 旧便签的 `text` / `todos` / `window` → 新 `notes[0]` 的内容与 `bounds`，标题固定为「便签」；
  `settings.alwaysOnTop` → `notes[0].pinned`；`settings.autostart` → 新 `settings.autostart`；
  若旧数据里已有 `style`（任务二版本）则继承，否则用默认奶油/浅色/中/97%
- 迁移成功后立即按 v2 落盘，因此重复启动不会再走迁移分支（幂等），也不会重复备份

## 每日自动备份

所有写盘都经过 `store::save`，备份就挂在这一处：**每天第一次落盘前**检查同目录有没有当天的
备份文件，没有就先把当前的 `store.json` 复制一份。

| 项 | 规则 |
| --- | --- |
| 文件名 | `store.YYYYMMDD.json`（本地日期，例如 `store.20261109.json`） |
| 位置 | 与 `store.json` 同目录（便携模式即 exe 同目录，不用 AppData / 当前工作目录） |
| 频率 | 每天最多一份；当天已存在就跳过 |
| 保留 | 最近 **7 份**，更早的自动删除（只认 `store.YYYYMMDD.json` 这种名字，不会碰 `store.v1.bak.json`） |
| 失败 | 只打印 `[store] 每日备份失败（忽略...）`，绝不阻断保存主流程 |

注意：

- 备份的是"今天第一次写盘**之前**"的内容，也就是昨天最后一次的状态——这才是误删 / 改坏时想找回的东西；
- 备份文件不参与启动读取，恢复时手动把它改回 `store.json` 再启动即可；
- 首次运行还没有 `store.json` 时不会产生备份。

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
