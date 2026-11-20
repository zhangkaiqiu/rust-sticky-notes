import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api } from "./api";
import type { TodoItem } from "./types";
import { normalizeStyle, styleVars, FONT_SIZES } from "./theme";
import type { NoteStyle } from "./theme";
import { textBasis, todoBasis } from "./split";
import TopBar, { BAR_HEIGHT } from "./components/TopBar";
import TodoList from "./components/TodoList";
import Splitter from "./components/Splitter";
import ContextMenu from "./components/ContextMenu";
import type { ContextMenuItem } from "./components/ContextMenu";
import { GearIcon, PinIcon } from "./components/icons";

function newId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export default function App() {
  const [ready, setReady] = useState(false);
  const [title, setTitle] = useState("便签");
  const [text, setText] = useState("");
  const [todos, setTodos] = useState<TodoItem[]>([]);
  const [pinned, setPinned] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  /**
   * 正文区 / 待办区的分割比例（待办区占比）。`0` = 用户没调过，按内容自适应。
   * 拖动过程中只改这个 state，松手才落盘（见 Splitter 的 onCommit）。
   */
  const [splitRatio, setSplitRatio] = useState(0);
  /** 分割线的比例基准：正文 + 待办这一整块（不含标题栏 / 错误提示） */
  const bodyRef = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<NoteStyle>(() => normalizeStyle(null));
  /** 设置面板是独立窗口，这里只记它开没开（由后端事件同步，不自己猜） */
  const [settingsOpen, setSettingsOpen] = useState(false);
  /**
   * 自定义右键菜单：坐标 + 触发位置。
   * - `field`：命中了输入框（正文 / 待办文字 / 添加待办），带文字编辑项；
   * - `card`：卡片空白处，只有便签级操作。
   * 没有 `title` 分支：标题栏整体是「不弹菜单」区（见 TopBar 的 data-no-menu）。
   */
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    target: "field" | "card";
  } | null>(null);
  /** 当前右键命中的输入元素，以及右键瞬间的选区（菜单点击会让输入框失焦） */
  const fieldRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null);
  const selectionRef = useRef<{ start: number; end: number } | null>(null);
  /** Ctrl+滚轮调字号的轻提示（卡片角落浮出 1 秒后淡出） */
  const [fontToast, setFontToast] = useState<{ id: number; label: string } | null>(null);
  const fontToastTimer = useRef<number | null>(null);
  const [savedTick, setSavedTick] = useState(0);
  const [error, setError] = useState<string | null>(null);
  /** 当前窗口所属便签的 id：跨窗口事件要靠它判断"是不是我自己的事" */
  const noteIdRef = useRef<string>("");
  /**
   * 正在接收远端同步。为 true 时外观防抖 effect 直接跳过，
   * 避免"远端更新 → 回写 → 再广播"的无限回环。
   */
  const syncingRef = useRef(false);

  // 1) 启动时只载入「当前窗口」对应的那张便签（label = note-<id>）
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const snapshot = await api.noteState();
        if (!alive) return;
        noteIdRef.current = snapshot.note.id;
        setTitle(snapshot.note.title || "便签");
        setText(snapshot.note.text);
        setTodos(snapshot.note.todos);
        setPinned(snapshot.note.pinned);
        setCollapsed(snapshot.note.collapsed === true);
        setSplitRatio(snapshot.note.splitRatio || 0);
        setStyle(normalizeStyle(snapshot.note));
        // 图钉图标以**窗口真实层级**为准：store 里的 pinned 只是期望值，
        // 启动时应用可能会被后续的层级操作覆盖（见 win_desktop_pin 顶部 BUG 2 说明）。
        // 只在这一刻回读一次，不做轮询。
        try {
          const actual = await api.alwaysOnTop();
          if (alive) setPinned(actual);
        } catch {
          // 查询失败就沿用 store 里的值，不阻塞启动
        }
      } catch (err) {
        if (alive) setError(`读取本地数据失败：${String(err)}`);
      } finally {
        if (alive) setReady(true);
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  // 2) 编辑防抖落盘（首次载入不回写）
  const skipFirstSave = useRef(true);
  useEffect(() => {
    if (!ready) return;
    if (skipFirstSave.current) {
      skipFirstSave.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      api
        .saveContent(text, todos)
        .then(() => {
          setSavedTick((n) => n + 1);
          setError(null);
        })
        .catch((err) => setError(`保存失败：${String(err)}`));
    }, 450);
    return () => window.clearTimeout(timer);
  }, [text, todos, ready]);

  // 3) 外观改动防抖落盘（拖动透明度滑块时实时预览，停手 300ms 后写盘）
  //    注意：收到远端同步（syncingRef）时**不**回写，否则会和广播形成无限回环。
  const skipFirstStyleSave = useRef(true);
  useEffect(() => {
    if (!ready) return;
    if (skipFirstStyleSave.current) {
      skipFirstStyleSave.current = false;
      return;
    }
    if (syncingRef.current) {
      syncingRef.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      api
        .saveStyle(style)
        .then(() => {
          setSavedTick((n) => n + 1);
          setError(null);
        })
        .catch((err) => setError(`保存外观失败：${String(err)}`));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [style, ready]);

  // 4) Ctrl + 滚轮调字号：上滚变大、下滚变小，到顶 / 到底停住（不回绕）。
  //    React 的 onWheel 在根节点上是 passive 的，preventDefault 无效，WebView2 会把整页缩放，
  //    所以必须用原生非 passive 监听。styleRef 保证监听只注册一次也能拿到最新字号。
  const showFontToast = useCallback((label: string) => {
    setFontToast({ id: Date.now(), label });
    if (fontToastTimer.current) window.clearTimeout(fontToastTimer.current);
    fontToastTimer.current = window.setTimeout(() => setFontToast(null), 1300);
  }, []);

  const styleRef = useRef(style);
  useEffect(() => {
    styleRef.current = style;
  }, [style]);

  useEffect(() => {
    if (!ready) return;

    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();

      const currentStyle = styleRef.current;
      const current = FONT_SIZES.findIndex((f) => f.id === currentStyle.fontSize);
      const index = current < 0 ? 1 : current;
      const nextIndex = event.deltaY < 0 ? index + 1 : index - 1;
      const clamped = Math.min(FONT_SIZES.length - 1, Math.max(0, nextIndex));
      if (clamped === index) return;

      const nextFontSize = FONT_SIZES[clamped];
      setStyle(normalizeStyle({ ...currentStyle, fontSize: nextFontSize.id }));
      showFontToast(nextFontSize.label);
    };

    // passive: false 才能 preventDefault，挡住 WebView2 自带的 Ctrl+滚轮缩放
    window.addEventListener("wheel", onWheel, { passive: false });
    return () => window.removeEventListener("wheel", onWheel);
  }, [ready, showFontToast]);

  // 4.1) 字号提示：1.3 秒后（含淡出动画）卸载
  useEffect(() => {
    return () => {
      if (fontToastTimer.current) window.clearTimeout(fontToastTimer.current);
    };
  }, []);

  // 5) 同步 HTML 文档标题：WebView2 会把 document.title 反映到窗口标题，
  // 这样 Alt+Tab / 任务管理器里显示的就是便签自己的名字。
  useEffect(() => {
    document.title = title;
  }, [title]);

  // 6) 错误提示自动消失
  useEffect(() => {
    if (!error) return;
    const t = window.setTimeout(() => setError(null), 5000);
    return () => window.clearTimeout(t);
  }, [error]);

  // 7) 跨窗口状态同步（见 Rust 侧 events 模块）
  //    - note-updated：另一端（设置面板）改了标题 / 外观 → 重新读一次
  //    - settings-state：面板开 / 关 → 同步齿轮按钮高亮
  //
  // ⚠️ 这里**必须**用 `syncing` 标记把回写挡掉：
  //    事件 → setStyle → 外观防抖 effect 以为用户改了外观 → saveStyle()
  //    → 后端广播 note-updated → 又 setStyle ……  这是一条无限回环，
  //    表现为"已保存"提示永远不消失（savedTick 被无限递增）。
  //    收到远端更新时只更新本地显示，绝不回写。
  useEffect(() => {
    let alive = true;

    const unlistenNote = listen<{ noteId: string }>("note-updated", (event) => {
      if (!alive || event.payload.noteId !== noteIdRef.current) return;
      void api
        .noteState()
        .then((snapshot) => {
          if (!alive) return;
          syncingRef.current = true;
          setTitle(snapshot.note.title || "便签");
          setStyle(normalizeStyle(snapshot.note));
        })
        .catch(() => {
          /* 单次读取失败不打扰用户，下次事件再同步 */
        });
    });

    const unlistenSettings = listen<{ noteId: string; open: boolean }>(
      "settings-state",
      (event) => {
        if (!alive || event.payload.noteId !== noteIdRef.current) return;
        setSettingsOpen(event.payload.open);
      }
    );

    return () => {
      alive = false;
      void unlistenNote.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
    };
  }, []);

  const addTodo = useCallback((raw: string) => {
    const value = raw.trim();
    if (!value) return;
    setTodos((prev) => [...prev, { id: newId(), text: value, done: false }]);
  }, []);

  const toggleTodo = useCallback((id: string) => {
    setTodos((prev) =>
      prev.map((item) => (item.id === id ? { ...item, done: !item.done } : item))
    );
  }, []);

  const editTodo = useCallback((id: string, next: string) => {
    setTodos((prev) =>
      prev.map((item) => (item.id === id ? { ...item, text: next } : item))
    );
  }, []);

  const removeTodo = useCallback((id: string) => {
    setTodos((prev) => prev.filter((item) => item.id !== id));
  }, []);

  const clearDone = useCallback(() => {
    setTodos((prev) => prev.filter((item) => !item.done));
  }, []);

  // ------------------------------------------------ 正文 / 待办分割

  /**
   * 拖动中：只改本地 state（每帧一次 setState 是廉价的），不落盘。
   * `0` 仍然是"自适应"哨兵，这里原样传下去。
   */
  const previewSplit = useCallback((ratio: number) => setSplitRatio(ratio), []);

  /**
   * 松手 / 键盘调整结束：落一次盘。
   *
   * 用后端的返回值回写 state（而不是用本地值）：归一化区间在 Rust 侧也有兜底，
   * 以后端为准就不会出现"界面 87%、JSON 里 82%"这种事。
   * 失败只提示，不把拖动结果弹回去——用户已经看到了效果，闪一下更难受。
   */
  const commitSplit = useCallback((ratio: number) => {
    void api
      .saveSplitRatio(ratio)
      .then((actual) => setSplitRatio(actual))
      .catch((err) => setError(`保存分区比例失败：${String(err)}`));
  }, []);

  /** 标题是离散操作（回车 / 失焦才提交），直接走同一条 Rust 单锁写盘通道 */
  const togglePin = useCallback(async () => {
    const next = !pinned;
    setPinned(next);
    try {
      // 后端回读 exstyle 后返回**真实**状态：图标跟着它走，
      // 避免"界面说钉住、窗口其实没钉住"的状态漂移。
      const actual = await api.setPinned(next);
      setPinned(actual);
      if (actual !== next) {
        setError(
          actual
            ? "置顶没有生效（窗口层级未被系统接受），已按实际状态显示"
            : "取消置顶没有生效，已按实际状态显示",
        );
      }
    } catch (err) {
      setPinned(!next);
      setError(`切换置顶失败：${String(err)}`);
    }
  }, [pinned]);

  /** 折叠成条 / 展开：窗口高度由 Rust 侧调整，前端只切内容与图标方向 */
  const toggleCollapsed = useCallback(async () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      await api.setCollapsed(next);
    } catch (err) {
      setCollapsed(!next);
      setError(`切换折叠失败：${String(err)}`);
    }
  }, [collapsed]);

  /**
   * 开关设置面板：面板是独立窗口，由后端创建 / 销毁。
   *
   * 关键点：**不拿本地 settingsOpen 做判断**。面板可能被用户点 X / Esc 关掉，
   * 本地状态就是过期的；以本地状态为准会出现"点一次没反应、要点第二次才开"。
   * 这里直接告诉后端"切换"，什么状态由后端说了算，结果通过事件回来更新按钮高亮。
   */
  const toggleSettings = useCallback(async () => {
    try {
      await api.toggleSettings();
    } catch (err) {
      setError(`切换设置面板失败：${String(err)}`);
    }
  }, []);

  // ------------------------------------------------ 自定义右键菜单

  /**
   * 右键只有**一条**规则，不再每个组件各写一遍 handler。
   *
   * 用 document 捕获阶段监听：命中输入框 → 编辑菜单（剪切/复制/粘贴/全选），
   * 其余位置 → 卡片菜单。这样正文、待办文字框、添加待办框共用同一套逻辑，
   * 以后再加输入框也不会漏（之前待办区就是因为各自手写 handler 才漏掉的）。
   *
   * `data-no-menu` 是例外标记，表示「这里不弹自定义菜单」：
   * - 标题栏：折叠态只有 40px 高，菜单必然被裁，且标题没有可操作项；
   * - 分割线：唯一动作是拖；
   * - 菜单自身：避免在菜单上再叠一层。
   * `data-menu-dismiss`（菜单的全屏遮罩）语义不同：右键只关闭，不在原地重开。
   * 无论哪种情况，WebView2 的原生菜单都一律 preventDefault 掉。
   */
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onContext = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target || !rootRef.current?.contains(target)) return;

      event.preventDefault();
      if (target.closest("[data-menu-dismiss]")) {
        setMenu(null);
        return;
      }
      if (collapsed || target.closest("[data-no-menu]")) return;

      const field = target.closest<HTMLInputElement | HTMLTextAreaElement>(
        "textarea, input"
      );
      if (field) {
        // 记录右键瞬间的元素与选区：菜单点击会让输入框失焦
        field.focus();
        const start = field.selectionStart ?? field.value.length;
        const end = field.selectionEnd ?? start;
        fieldRef.current = field;
        selectionRef.current = { start, end };
        setMenu({ x: event.clientX, y: event.clientY, target: "field" });
      } else {
        fieldRef.current = null;
        selectionRef.current = null;
        setMenu({ x: event.clientX, y: event.clientY, target: "card" });
      }
    };

    document.addEventListener("contextmenu", onContext);
    return () => document.removeEventListener("contextmenu", onContext);
  }, [collapsed]);

  const closeMenu = useCallback(() => setMenu(null), []);

  const readSelection = useCallback(() => {
    const el = fieldRef.current;
    const sel = selectionRef.current;
    if (!el || !sel) return null;
    const start = Math.min(sel.start, sel.end);
    const end = Math.max(sel.start, sel.end);
    return { el, start, end, selected: el.value.slice(start, end) };
  }, []);

  const copySelection = useCallback(async () => {
    const info = readSelection();
    if (!info || !info.selected) return;
    try {
      await writeText(info.selected);
    } catch (err) {
      setError(`复制失败：${String(err)}`);
    }
  }, [readSelection]);

  const cutSelection = useCallback(async () => {
    const info = readSelection();
    if (!info || !info.selected) return;
    try {
      await writeText(info.selected);
      info.el.focus();
      info.el.setSelectionRange(info.start, info.end);
      // 走编辑命令删除，浏览器撤销栈（Ctrl+Z）仍然可用
      document.execCommand("delete");
    } catch (err) {
      setError(`剪切失败：${String(err)}`);
    }
  }, [readSelection]);

  const pasteAtSelection = useCallback(async () => {
    const info = readSelection();
    if (!info) return;
    try {
      const text = await readText();
      if (!text) return;
      info.el.focus();
      info.el.setSelectionRange(info.start, info.end);
      // 优先 execCommand('insertText')：在光标处插入且保持撤销栈
      if (!document.execCommand("insertText", false, text)) {
        // 兜底：用原型上的 value setter 改值再派发 input，React 的 onChange 才能收到
        const proto =
          info.el instanceof HTMLTextAreaElement
            ? HTMLTextAreaElement.prototype
            : HTMLInputElement.prototype;
        const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
        setter?.call(
          info.el,
          info.el.value.slice(0, info.start) + text + info.el.value.slice(info.end)
        );
        info.el.dispatchEvent(new Event("input", { bubbles: true }));
      }
    } catch (err) {
      setError(`粘贴失败：${String(err)}`);
    }
  }, [readSelection]);

  const selectAllInField = useCallback(() => {
    const el = fieldRef.current;
    if (!el) return;
    el.focus();
    el.select();
  }, []);

  const createNote = useCallback(() => {
    void api.createNote().catch((err) => setError(`新建便签失败：${String(err)}`));
  }, []);

  // 打开菜单时算一次选中内容，用来决定剪切 / 复制是否可用
  const contextSelection = useMemo(() => {
    if (!menu || menu.target !== "field") return "";
    return readSelection()?.selected ?? "";
  }, [menu, readSelection]);

  const contextMenuItems: ContextMenuItem[] = useMemo(() => {
    if (!menu) return [];

    // 右键菜单只放**别处没有入口**的操作：
    // 折叠 / 设置 / 置顶 / 删除各自有专属按钮或设置面板，
    // 这里再列一遍就是重复入口，只留新建便签。
    const newNote: ContextMenuItem = {
      id: "new-note",
      label: "新建便签",
      onSelect: createNote,
    };

    // 卡片空白处：只有便签级操作。单条时不要再顶一条分隔线（那会像"菜单坏了"）
    if (menu.target === "card") return [newNote];

    return [
      {
        id: "cut",
        label: "剪切",
        shortcut: "Ctrl+X",
        disabled: !contextSelection,
        onSelect: () => void cutSelection(),
      },
      {
        id: "copy",
        label: "复制",
        shortcut: "Ctrl+C",
        disabled: !contextSelection,
        onSelect: () => void copySelection(),
      },
      {
        id: "paste",
        label: "粘贴",
        shortcut: "Ctrl+V",
        onSelect: () => void pasteAtSelection(),
      },
      { id: "select-all", label: "全选", shortcut: "Ctrl+A", onSelect: selectAllInField },
      { kind: "separator", id: "sep-actions" },
      newNote,
    ];
  }, [
    menu,
    contextSelection,
    createNote,
    cutSelection,
    copySelection,
    pasteAtSelection,
    selectAllInField,
  ]);

  const vars = useMemo(() => styleVars(style), [style]);
  // 低透明度时补一圈极轻的内描边，避免卡片"只剩文字"
  const lowOpacity = style.opacity < 0.6;

  return (
    <div
      ref={rootRef}
      className="note-root relative h-full w-full"
      style={vars}
    >
      {/*
        图钉 / 设置按钮故意放在卡片**外面**：它们绝对定位、不参与卡片内的 flex，
        便签被拖窄时也不会被挤出去、更不会被卡片的 overflow-hidden 裁掉，
        视觉上就是浮在便签右上角。

        高度用固定的 BAR_HEIGHT，不用 `inset-y-0` 也不用写死 36px：
        这组按钮和标题栏必须是同一个高度、同一个垂直中心，否则折叠切换时会跳。
      */}
      <div
        style={{ height: BAR_HEIGHT }}
        className="pointer-events-none absolute right-1 top-0 z-20 flex items-center gap-0.5"
      >
        {/*
          图钉只有两种视觉状态，不再画外圈 / 底色块：
          - 未置顶：竖直、淡色，hover 变深；
          - 置顶：整体旋转 45°（斜着的图钉），保持深色。
          旋转加在 svg 自身上（transform-origin 居中），按钮 padding 留白足够、
          且没有 overflow-hidden，所以旋转后不会被裁切，点击热区也不变。
        */}
        <button
          type="button"
          aria-label={pinned ? "取消置顶" : "置顶"}
          aria-pressed={pinned}
          onClick={togglePin}
          onMouseDown={(e) => e.stopPropagation()}
          className="note-icon-btn note-focus pointer-events-auto group flex items-center justify-center rounded-full p-[7px] focus-visible:outline-none"
        >
          <PinIcon
            className={`h-[0.9375em] w-[0.9375em] overflow-visible transition-[transform,color] duration-200 ease-out ${
              pinned ? "rotate-45" : ""
            }`}
          />
        </button>

        <button
          type="button"
          aria-label="设置"
          aria-expanded={settingsOpen}
          onClick={toggleSettings}
          onMouseDown={(e) => e.stopPropagation()}
          className={`note-focus pointer-events-auto rounded-full p-[7px] focus-visible:outline-none ${
            settingsOpen ? "note-icon-btn-active transition-colors" : "note-icon-btn"
          }`}
        >
          <GearIcon className="h-[0.9375em] w-[0.9375em]" />
        </button>
      </div>

      <div
        className={`note-card relative flex h-full w-full flex-col overflow-hidden rounded-[18px] border ${
          lowOpacity ? "note-card-edge" : ""
        }`}
      >
        <TopBar
          title={title}
          collapsed={collapsed}
          savedTick={savedTick}
          onToggleCollapsed={toggleCollapsed}
        />

        {/* 折叠成条时只留标题栏：正文 / 待办都不渲染（窗口高度只有 40 逻辑像素） */}
        {!collapsed && (
          <>
            {/*
              正文 + 待办 + 分割线是**一整块**（bodyRef）：分割比例以它的高度为基准，
              所以窗口拉高拉低时两个区域一起缩放，不需要任何窗口尺寸监听。

              两种布局（互斥，不叠加）：
              - splitRatio = 0：自适应。正文 flex-1、待办按内容撑开最多 44%（改造前的样子）；
              - splitRatio > 0：手动。两边都是固定 flex-basis，百分比里已经扣掉分割线高度，
                所以两者加起来正好铺满，不会因为多出 9px 把某一区挤变形。
            */}
            <div ref={bodyRef} className="flex min-h-0 flex-1 flex-col">
              <textarea
                value={text}
                onChange={(e) => setText(e.target.value)}
                spellCheck={false}
                placeholder="写点什么…"
                style={
                  splitRatio > 0 ? { flexBasis: textBasis(splitRatio) } : undefined
                }
                className={`note-placeholder note-text note-scroll min-h-[72px] resize-none bg-transparent px-4 pb-1 pt-0.5 text-[0.84375em] leading-relaxed outline-none ${
                  // 手动分区：basis 就是用户拖出来的高度，不许被 flex 再压一次；
                  // 自适应：正文吃掉待办之外的全部空间
                  splitRatio > 0 ? "shrink-0" : "flex-1"
                }`}
              />

              <Splitter
                ratio={splitRatio}
                bodyRef={bodyRef}
                onChange={previewSplit}
                onCommit={commitSplit}
              />

              <TodoList
                todos={todos}
                onAdd={addTodo}
                onToggle={toggleTodo}
                onEdit={editTodo}
                onRemove={removeTodo}
                onClearDone={clearDone}
                splitBasis={splitRatio > 0 ? todoBasis(splitRatio) : null}
              />
            </div>

            {error && (
              <div className="note-danger px-4 pb-2 text-[0.6875em] leading-snug">
                {error}
              </div>
            )}
          </>
        )}

        {/* Ctrl+滚轮调字号的轻提示：角落浮出约 1 秒后淡出，不用 alert */}
        {fontToast && (
          <div
            key={fontToast.id}
            className="note-toast note-subtle-bg-strong note-text pointer-events-none absolute bottom-3 right-3 rounded-full px-2.5 py-1 text-[0.6875em] shadow-sm"
          >
            字号：{fontToast.label}
          </div>
        )}
      </div>

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={contextMenuItems} onClose={closeMenu} />
      )}
    </div>
  );
}
