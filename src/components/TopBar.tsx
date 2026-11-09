import { useEffect, useRef, useState } from "react";
import { ChevronIcon, GearIcon, PinIcon } from "./icons";

type Props = {
  title: string;
  pinned: boolean;
  collapsed: boolean;
  savedTick: number;
  settingsOpen: boolean;
  onTitleChange: (next: string) => void;
  onTogglePin: () => void;
  onToggleCollapsed: () => void;
  onToggleSettings: () => void;
};

/** 与 Rust 侧 `store::MAX_TITLE_CHARS` 保持一致 */
const MAX_TITLE_CHARS = 60;
const DEFAULT_TITLE = "便签";

/** 顶部条既是拖动区，也放标题 / 图钉 / 设置按钮 */
export default function TopBar({
  title,
  pinned,
  collapsed,
  savedTick,
  settingsOpen,
  onTitleChange,
  onTogglePin,
  onToggleCollapsed,
  onToggleSettings,
}: Props) {
  const [justSaved, setJustSaved] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (savedTick === 0) return;
    setJustSaved(true);
    const t = window.setTimeout(() => setJustSaved(false), 1400);
    return () => window.clearTimeout(t);
  }, [savedTick]);

  // 进入编辑态后聚焦并全选，方便直接覆盖旧标题
  useEffect(() => {
    if (!editing) return;
    const el = inputRef.current;
    if (el) {
      el.focus();
      el.select();
    }
  }, [editing]);

  const beginEdit = () => {
    setDraft(title);
    setEditing(true);
  };

  const commit = () => {
    if (!editing) return;
    const next = draft.trim().slice(0, MAX_TITLE_CHARS) || DEFAULT_TITLE;
    setEditing(false);
    setDraft(next);
    if (next !== title) onTitleChange(next);
  };

  const cancel = () => {
    setEditing(false);
    setDraft(title);
  };

  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center gap-1 pl-1.5 pr-1.5"
    >
      {/*
        折叠 / 展开箭头：单独一个按钮，mousedown 阻止冒泡，
        这样点它只切换折叠，不会误触标题栏的拖拽（旁边空白仍可拖动）。
      */}
      <button
        type="button"
        title={collapsed ? "展开便签" : "折叠成条"}
        aria-expanded={!collapsed}
        onClick={onToggleCollapsed}
        onMouseDown={(e) => e.stopPropagation()}
        className="note-icon-btn note-focus flex shrink-0 items-center justify-center rounded-full p-[5px] focus-visible:outline-none"
      >
        <ChevronIcon
          className={`h-[0.875em] w-[0.875em] transition-transform duration-200 ${
            collapsed ? "rotate-180" : ""
          }`}
        />
      </button>

      {editing ? (
        /*
          输入框不挂 data-tauri-drag-region，且 mousedown 阻止冒泡：
          点标题能正常聚焦 / 选词，点它旁边的空白仍然拖窗口。
        */
        <input
          ref={inputRef}
          value={draft}
          maxLength={MAX_TITLE_CHARS}
          aria-label="便签标题"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onMouseDown={(e) => e.stopPropagation()}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") {
              e.preventDefault();
              commit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              cancel();
            }
          }}
          style={{ width: `${Math.min(14, Math.max(3, draft.length + 1))}ch` }}
          className="note-title-input text-[0.625em] font-semibold tracking-[0.14em] outline-none"
        />
      ) : (
        <button
          type="button"
          title={title}
          onClick={beginEdit}
          onMouseDown={(e) => e.stopPropagation()}
          className="note-title-btn note-focus truncate rounded-[3px] px-0.5 text-[0.625em] font-semibold uppercase tracking-[0.14em] focus-visible:outline-none"
        >
          {title}
        </button>
      )}

      {/* 剩余空白都可拖动：pointer-events-none 让事件落到父级拖动区 */}
      <div data-tauri-drag-region className="pointer-events-none h-full flex-1" />

      {/* 折叠成条时空间紧张，保存提示让位给标题（保存状态仍照常落盘） */}
      {!collapsed && (
        <span
          className={`note-faint mr-0.5 text-[0.625em] transition-opacity duration-300 ${
            justSaved ? "opacity-100" : "opacity-0"
          }`}
        >
          已保存
        </span>
      )}

      {/*
        图钉只有两种视觉状态，不再画外圈 / 底色块：
        - 未置顶：竖直、淡色，hover 变深；
        - 置顶：整体旋转 45°（斜着的图钉），保持深色。
        旋转加在 svg 自身上（transform-origin 居中），按钮 padding 留白足够、
        且没有 overflow-hidden，所以旋转后不会被裁切，点击热区也不变。
      */}
      <button
        type="button"
        title={pinned ? "取消置顶" : "置顶"}
        aria-pressed={pinned}
        onClick={onTogglePin}
        className="note-icon-btn note-focus group flex items-center justify-center rounded-full p-[7px] focus-visible:outline-none"
      >
        <PinIcon
          className={`h-[0.9375em] w-[0.9375em] overflow-visible transition-[transform,color] duration-200 ease-out ${
            pinned ? "rotate-45" : ""
          }`}
        />
      </button>

      <button
        type="button"
        title="设置"
        aria-expanded={settingsOpen}
        onClick={onToggleSettings}
        className={`note-focus rounded-full p-[7px] focus-visible:outline-none ${
          settingsOpen
            ? "note-icon-btn-active transition-colors"
            : "note-icon-btn"
        }`}
      >
        <GearIcon className="h-[0.9375em] w-[0.9375em]" />
      </button>
    </div>
  );
}
