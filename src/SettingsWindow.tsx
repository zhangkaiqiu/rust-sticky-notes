import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { normalizeStyle, styleVars } from "./theme";
import type { NoteStyle } from "./theme";
import SettingsPanel from "./components/SettingsPanel";

/**
 * 设置面板窗口的根组件。
 *
 * 它跑在 Rust 侧创建的独立窗口里（`settings-<noteId>`），所以：
 * - 不需要遮罩层，也不需要 `fixed` 定位——窗口本身就是面板；
 * - 不受便签尺寸影响，折叠成条时照样完整显示；
 * - 永远不出现滚动条（窗口高度就是内容高度）。
 *
 * 数据一致性：监听 `note-updated` 事件。便签窗口改了标题 / 外观时，
 * 面板会收到通知并重新读一次，避免"面板里显示的是旧值"。
 */
export default function SettingsWindow({ noteId }: { noteId: string }) {
  const [title, setTitle] = useState("便签");
  const [style, setStyle] = useState<NoteStyle>(() => normalizeStyle(null));
  const [storePath, setStorePath] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const vars = styleVars(style);

  /** 从后端读一次当前便签状态（后端按窗口 label 解析到这张便签） */
  const reload = useCallback(async () => {
    const snapshot = await api.noteState();
    setTitle(snapshot.note.title || "便签");
    setStyle(normalizeStyle(snapshot.note));
    setStorePath(snapshot.storePath);
  }, []);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        await reload();
      } catch (err) {
        if (alive) setError(`读取便签失败：${String(err)}`);
      } finally {
        if (alive) setReady(true);
      }
    })();
    return () => {
      alive = false;
    };
  }, [reload]);

  // 另一侧（便签窗口）改了数据 → 重新读，避免面板显示旧值
  useEffect(() => {
    const unlisten = listen<{ noteId: string }>("note-updated", (event) => {
      if (event.payload.noteId !== noteId) return;
      void reload().catch(() => {
        /* 单次读取失败不打扰用户，下次事件会再同步 */
      });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [noteId, reload]);

  /** 外观改动：本地即时预览，同时写回后端（便签窗口会收到事件并刷新） */
  const updateStyle = useCallback((patch: Partial<NoteStyle>) => {
    setStyle((prev) => {
      const next = normalizeStyle({ ...prev, ...patch });
      void api.saveStyle(next).catch((err) => setError(`保存外观失败：${String(err)}`));
      return next;
    });
  }, []);

  /** 标题改名：面板是唯一的改名入口（便签栏上的标题只负责显示和拖动） */
  const onTitleChange = useCallback((next: string) => setTitle(next), []);

  /** 提交标题：失焦 / 回车才写盘，避免每敲一个字就落一次盘 */
  const commitTitle = useCallback((raw: string) => {
    void api
      .saveTitle(raw)
      .then((actual) => setTitle(actual))
      .catch((err) => setError(`保存标题失败：${String(err)}`));
  }, []);

  const close = useCallback(() => {
    void api.closeSettings();
  }, []);

  /**
   * 内容高度变了 → 让窗口跟着变。
   *
   * 为什么不是写死窗口高度：面板内容会随便签字号、标题换行、控件增删变化，
   * 写死迟早对不上——不是裁掉底部，就是留一大块空白。
   * 这里做防抖，避免 ResizeObserver 的连续回调把窗口抖动得很频繁。
   */
  const resizeTimer = useRef<number | null>(null);
  const handleResize = useCallback((contentHeight: number) => {
    if (resizeTimer.current) window.clearTimeout(resizeTimer.current);
    resizeTimer.current = window.setTimeout(() => {
      void api.resizeSettings(contentHeight).catch(() => {
        /* 调整失败不打扰用户：面板内容自己还能滚动兜底 */
      });
    }, 40);
  }, []);

  useEffect(() => {
    return () => {
      if (resizeTimer.current) window.clearTimeout(resizeTimer.current);
    };
  }, []);

  const hideToTray = useCallback(() => {
    // 面板是便签的附属窗口，后端会一并收起
    void api.hideNote();
  }, []);

  const deleteNote = useCallback(() => {
    setConfirmDelete(false);
    void api.deleteNote().catch((err) => setError(`删除便签失败：${String(err)}`));
  }, []);

  // Esc 关面板
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

  if (!ready) {
    // 载入极快，这里只在首帧出现，避免闪一下白底
    return <div className="note-root h-full w-full" style={vars} />;
  }

  return (
    <div className="h-full w-full" style={vars}>
      <SettingsPanel
        title={title}
        storePath={storePath}
        style={style}
        onStyleChange={updateStyle}
        onTitleInput={onTitleChange}
        onTitleCommit={commitTitle}
        onHide={hideToTray}
        onDelete={deleteNote}
        onClose={close}
        onResize={handleResize}
        confirmDelete={confirmDelete}
        onConfirmDeleteChange={setConfirmDelete}
      />
      {error && (
        <div className="note-danger pointer-events-none absolute bottom-2 left-2 right-2 text-[0.625em] leading-snug">
          {error}
        </div>
      )}
    </div>
  );
}
