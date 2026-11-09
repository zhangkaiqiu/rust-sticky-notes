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
  const [text, setText] = useState("");
  const [todos, setTodos] = useState<TodoItem[]>([]);
  const [pinned, setPinned] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [style, setStyle] = useState<NoteStyle>(() => normalizeStyle(null));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [storePath, setStorePath] = useState("");
  const [savedTick, setSavedTick] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // 1) 启动时从 Rust 侧载入整份状态
  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const state = await api.loadState();
        if (!alive) return;
        setText(state.text);
        setTodos(state.todos);
        setPinned(state.settings.alwaysOnTop);
        setAutostart(state.settings.autostart);
        setStyle(normalizeStyle(state.style));
        setStorePath(await api.getStorePath());
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
        .saveNote(text, todos)
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

  // 5) 错误提示自动消失
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
    void api.hideWindow();
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
          pinned={pinned}
          savedTick={savedTick}
          settingsOpen={settingsOpen}
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
          onQuit={quit}
          onClose={() => setSettingsOpen(false)}
        />
      )}
    </div>
  );
}
