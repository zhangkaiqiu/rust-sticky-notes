import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import type { TodoItem } from "./types";
import { normalizeStyle, styleVars } from "./theme";
import type { NoteStyle } from "./theme";
import TopBar from "./components/TopBar";
import TodoList from "./components/TodoList";
import SettingsPanel from "./components/SettingsPanel";

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
  const [autostart, setAutostart] = useState(false);
  const [style, setStyle] = useState<NoteStyle>(() => normalizeStyle(null));
  const [settingsOpen, setSettingsOpen] = useState(false);
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
        setAutostart(snapshot.autostart);
        setStyle(normalizeStyle(snapshot.note));
        setStorePath(snapshot.storePath);
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

  // 4) Esc 关闭设置面板
  useEffect(() => {
    if (!settingsOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSettingsOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsOpen]);

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
      await api.setPinned(next);
    } catch (err) {
      setPinned(!next);
      setError(`切换置顶失败：${String(err)}`);
    }
  }, [pinned]);

  const toggleAutostart = useCallback(async () => {
    try {
      const actual = await api.setAutostart(!autostart);
      setAutostart(actual);
    } catch (err) {
      setError(`切换开机自启失败：${String(err)}`);
    }
  }, [autostart]);

  const hideToTray = useCallback(() => {
    setSettingsOpen(false);
    void api.hideNote();
  }, []);

  /** 删除当前便签：后端先落盘再销毁窗口；删到一张不剩时应用会自行退出 */
  const deleteThisNote = useCallback(() => {
    setSettingsOpen(false);
    void api.deleteNote().catch((err) => setError(`删除便签失败：${String(err)}`));
  }, []);

  const quit = useCallback(() => {
    void api.quitApp();
  }, []);

  const vars = useMemo(() => styleVars(style), [style]);
  // 低透明度时补一圈极轻的内描边，避免卡片"只剩文字"
  const lowOpacity = style.opacity < 0.6;

  return (
    <div className="note-root h-full w-full" style={vars}>
      <div
        className={`note-card relative flex h-full w-full flex-col overflow-hidden rounded-[18px] border ${
          lowOpacity ? "note-card-edge" : ""
        }`}
      >
        <TopBar
          title={title}
          pinned={pinned}
          savedTick={savedTick}
          settingsOpen={settingsOpen}
          onTitleChange={renameTitle}
          onTogglePin={togglePin}
          onToggleSettings={() => setSettingsOpen((v) => !v)}
        />

        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
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
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
