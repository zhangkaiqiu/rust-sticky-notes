import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { TodoItem } from "./types";
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

  // 3) Esc 关闭设置面板
  useEffect(() => {
    if (!settingsOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSettingsOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsOpen]);

  // 4) 错误提示自动消失
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

  return (
    <div className="h-full w-full">
      <div className="relative flex h-full w-full flex-col overflow-hidden rounded-[18px] border border-amber-900/15 bg-[#FFF8D6]/[0.97]">
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
          className="min-h-[72px] flex-1 resize-none bg-transparent px-4 pb-1 pt-0.5 text-[13.5px] leading-relaxed text-stone-800 outline-none placeholder:text-stone-500/50"
        />

        <div className="mx-3 h-px shrink-0 bg-amber-900/10" />

        <TodoList
          todos={todos}
          onAdd={addTodo}
          onToggle={toggleTodo}
          onEdit={editTodo}
          onRemove={removeTodo}
          onClearDone={clearDone}
        />

        {error && (
          <div className="px-4 pb-2 text-[11px] leading-snug text-red-600/90">
            {error}
          </div>
        )}
      </div>

      {settingsOpen && (
        <SettingsPanel
          pinned={pinned}
          autostart={autostart}
          storePath={storePath}
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
