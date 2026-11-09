import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { api } from "./api";
import type { TodoItem } from "./types";
import { normalizeStyle, styleVars, FONT_SIZES } from "./theme";
import type { NoteStyle } from "./theme";
import TopBar from "./components/TopBar";
import TodoList from "./components/TodoList";
import SettingsPanel from "./components/SettingsPanel";
import ContextMenu from "./components/ContextMenu";
import type { ContextMenuItem } from "./components/ContextMenu";

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
  const [autostart, setAutostart] = useState(false);
  const [style, setStyle] = useState<NoteStyle>(() => normalizeStyle(null));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  /** 自定义右键菜单：坐标 + 触发位置（输入框 / 卡片空白） */
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
  const [storePath, setStorePath] = useState("");
  const [savedTick, setSavedTick] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // 1) 启动时只载入「当前窗口」对应的那张便签（label = note-<id>）
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const snapshot = await api.noteState();
        if (!alive) return;
        setTitle(snapshot.note.title || "便签");
        setText(snapshot.note.text);
        setTodos(snapshot.note.todos);
        setPinned(snapshot.note.pinned);
        setCollapsed(snapshot.note.collapsed === true);
        setAutostart(snapshot.autostart);
        setStyle(normalizeStyle(snapshot.note));
        setStorePath(snapshot.storePath);
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
  const skipFirstStyleSave = useRef(true);
  useEffect(() => {
    if (!ready) return;
    if (skipFirstStyleSave.current) {
      skipFirstStyleSave.current = false;
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

  // 5) Esc 关闭设置面板
  useEffect(() => {
    if (!settingsOpen) return;
    // 右键菜单打开时，Esc 先关菜单（ContextMenu 在捕获阶段处理），这里让路
    if (menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSettingsOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsOpen, menu]);

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

  const updateStyle = useCallback((patch: Partial<NoteStyle>) => {
    setStyle((prev) => normalizeStyle({ ...prev, ...patch }));
  }, []);

  /** 标题是离散操作（回车 / 失焦才提交），直接走同一条 Rust 单锁写盘通道 */
  const renameTitle = useCallback((next: string) => {
    setTitle(next);
    api
      .saveTitle(next)
      .then((actual) => setTitle(actual))
      .catch((err) => setError(`保存标题失败：${String(err)}`));
  }, []);

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
    // 折叠后设置面板没有立足之地，顺手关掉
    setSettingsOpen(false);
    try {
      await api.setCollapsed(next);
    } catch (err) {
      setCollapsed(!next);
      setError(`切换折叠失败：${String(err)}`);
    }
  }, [collapsed]);

  const toggleAutostart = useCallback(async () => {    try {
      const actual = await api.setAutostart(!autostart);
      setAutostart(actual);
    } catch (err) {
      setError(`切换开机自启失败：${String(err)}`);
    }
  }, [autostart]);

  const hideToTray = useCallback(() => {
    setSettingsOpen(false);
    setConfirmDelete(false);
    void api.hideNote();
  }, []);

  /** 删除当前便签：后端先落盘再销毁窗口；删到一张不剩时应用会自行退出 */
  const deleteThisNote = useCallback(() => {
    setSettingsOpen(false);
    setConfirmDelete(false);
    void api.deleteNote().catch((err) => setError(`删除便签失败：${String(err)}`));
  }, []);

  const quit = useCallback(() => {
    void api.quitApp();
  }, []);

  // ------------------------------------------------ 自定义右键菜单

  const openFieldMenu = useCallback((event: ReactMouseEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    const el = event.currentTarget as HTMLInputElement | HTMLTextAreaElement;
    el.focus();
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? start;
    fieldRef.current = el;
    selectionRef.current = { start, end };
    setMenu({ x: event.clientX, y: event.clientY, target: "field" });
  }, []);

  const openCardMenu = useCallback((event: ReactMouseEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    setMenu({ x: event.clientX, y: event.clientY, target: "card" });
  }, []);

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

  const requestDelete = useCallback(() => {
    // 复用设置面板的二次确认：打开面板并直接进入"确认删除"态
    setSettingsOpen(true);
    setConfirmDelete(true);
  }, []);

  // 打开菜单时算一次选中内容，用来决定剪切 / 复制是否可用
  const contextSelection = useMemo(() => {
    if (!menu || menu.target !== "field") return "";
    return readSelection()?.selected ?? "";
  }, [menu, readSelection]);

  const contextMenuItems: ContextMenuItem[] = useMemo(() => {
    if (!menu) return [];
    const common: ContextMenuItem[] = [
      { kind: "separator", id: "sep-actions" },
      { id: "new-note", label: "新建便签", onSelect: createNote },
      {
        id: "collapse",
        label: collapsed ? "展开便签" : "折叠成条",
        onSelect: () => void toggleCollapsed(),
      },
      { id: "settings", label: "设置…", onSelect: () => setSettingsOpen(true) },
      { kind: "separator", id: "sep-delete" },
      { id: "delete", label: "删除便签", danger: true, onSelect: requestDelete },
    ];

    if (menu.target === "card") return common;

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
      ...common,
    ];
  }, [
    menu,
    contextSelection,
    collapsed,
    createNote,
    toggleCollapsed,
    requestDelete,
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
      className="note-root h-full w-full"
      style={vars}
      // 兜底：别让 WebView2 的原生右键菜单冒出来（卡片 / 输入框各自有自己的处理）
      onContextMenu={(e) => e.preventDefault()}
    >
      <div
        className={`note-card relative flex h-full w-full flex-col overflow-hidden rounded-[18px] border ${
          lowOpacity ? "note-card-edge" : ""
        }`}
        onContextMenu={openCardMenu}
      >
        <TopBar
          title={title}
          pinned={pinned}
          collapsed={collapsed}
          savedTick={savedTick}
          settingsOpen={settingsOpen}
          onTitleChange={renameTitle}
          onTogglePin={togglePin}
          onToggleCollapsed={toggleCollapsed}
          onToggleSettings={() => setSettingsOpen((v) => !v)}
          onFieldContextMenu={openFieldMenu}
        />

        {/* 折叠成条时只留标题栏：正文 / 待办都不渲染（窗口高度只有 48 逻辑像素） */}
        {!collapsed && (
          <>
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              onContextMenu={openFieldMenu}
              spellCheck={false}
              placeholder="写点什么…"
              className="note-placeholder note-text note-scroll min-h-[72px] flex-1 resize-none bg-transparent px-4 pb-1 pt-0.5 text-[0.84375em] leading-relaxed outline-none"
            />

            <div className="note-divider mx-3 h-px shrink-0" />

            <TodoList
              todos={todos}
              onAdd={addTodo}
              onToggle={toggleTodo}
              onEdit={editTodo}
              onRemove={removeTodo}
              onClearDone={clearDone}
            />

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

      {settingsOpen && (
        <SettingsPanel
          pinned={pinned}
          autostart={autostart}
          storePath={storePath}
          style={style}
          onStyleChange={updateStyle}
          onTogglePin={togglePin}
          onToggleAutostart={toggleAutostart}
          onHide={hideToTray}
          onDelete={deleteThisNote}
          onQuit={quit}
          confirmDelete={confirmDelete}
          onConfirmDeleteChange={setConfirmDelete}
          onClose={() => {
            setSettingsOpen(false);
            setConfirmDelete(false);
          }}
        />
      )}

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={contextMenuItems} onClose={closeMenu} />
      )}
    </div>
  );
}
