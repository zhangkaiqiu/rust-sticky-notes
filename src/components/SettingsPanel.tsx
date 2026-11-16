import type { CSSProperties } from "react";
import { useEffect, useRef } from "react";
import {
  FONT_SIZES,
  MAX_OPACITY,
  MIN_OPACITY,
  NOTE_COLORS,
  PALETTES,
  swatchStyle,
} from "../theme";
import type { NoteColor, NoteStyle, NoteTheme } from "../theme";
import { TrashIcon, TrayIcon } from "./icons";

/**
 * 便签设置面板 —— 运行在**独立的设置窗口**里（见 Rust 侧 `settings_panel` 模块）。
 *
 * 因为是独立窗口，它不受便签尺寸约束：便签折叠成 48px 的条、或拖到最窄 240px，
 * 面板依然完整显示，永远不需要滚动条。
 *
 * 面板里只放**这张便签**的东西（外观 / 置顶 / 隐藏 / 删除）；
 * 「开机自启」「退出应用」这类全局操作属于托盘菜单，不在这里。
 */
type Props = {
  title: string;
  storePath: string;
  style: NoteStyle;
  onStyleChange: (patch: Partial<NoteStyle>) => void;
  /** 标题输入中（只更新本地显示，不落盘） */
  onTitleInput: (next: string) => void;
  /** 标题提交（失焦 / 回车才落盘） */
  onTitleCommit: (next: string) => void;
  onHide: () => void;
  onDelete: () => void;
  onClose: () => void;
  /** 内容真实高度（CSS 像素）：让窗口跟着内容走，避免裁切或留大片空白 */
  onResize: (contentHeight: number) => void;
  /** 删除二次确认的展开状态 */
  confirmDelete: boolean;
  onConfirmDeleteChange: (next: boolean) => void;
};

/** 与 Rust 侧 `store::MAX_TITLE_CHARS` 保持一致 */
const MAX_TITLE_CHARS = 60;

/** 小号分段选择器（字号 / 主题） */
function Segmented<T extends string>({
  value,
  options,
  ariaLabel,
  onChange,
}: {
  value: T;
  options: { id: T; label: string }[];
  ariaLabel: string;
  onChange: (id: T) => void;
}) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className="note-subtle-bg flex gap-0.5 rounded-lg p-0.5"
    >
      {options.map((option) => {
        const selected = option.id === value;
        return (
          <button
            key={option.id}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(option.id)}
            className={`note-focus rounded-md px-1.5 py-[2px] text-[0.6875em] transition-colors focus-visible:outline-none ${
              selected ? "note-accent-bg shadow-sm" : "note-muted note-hover-soft"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

const THEME_OPTIONS: { id: NoteTheme; label: string }[] = [
  { id: "light", label: "浅色" },
  { id: "dark", label: "深色" },
];

export default function SettingsPanel({
  title,
  storePath,
  style,
  onStyleChange,
  onTitleInput,
  onTitleCommit,
  onHide,
  onDelete,
  onClose,
  onResize,
  confirmDelete,
  onConfirmDeleteChange,
}: Props) {
  const palette = PALETTES[style.color][style.theme];
  // 便签本身可以很透，但设置面板必须保持可读，最低按 90% 不透明度显示
  const panelStyle = {
    "--note-panel-alpha": Math.max(style.opacity, 0.9).toFixed(2),
  } as CSSProperties;

  const selectColor = (color: NoteColor) => onStyleChange({ color });

  // 标题编辑：输入时只改本地显示，失焦 / 回车才落盘
  const commitTitle = () => {
    const next = title.trim().slice(0, MAX_TITLE_CHARS) || "便签";
    onTitleCommit(next);
  };

  /*
    面板高度由**内容**决定，不写死。
    内容高度会随便签字号、标题换行、以及将来增删控件而变化，
    Rust 侧写死的窗口高度迟早对不上——不是裁掉底部（数据文件看不到），
    就是留一大块空白。这里量一次真实高度报给后端，窗口跟着内容走。
  */
  const contentRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;
    const report = () => onResize(el.getBoundingClientRect().height);
    report();
    // 字号 / 标题换行都会改变高度，跟着观察
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [onResize]);

  return (
    // 面板自己就是整个窗口，内层按内容自然撑高（不写 h-full，也不滚动）
    <div className="note-root w-full" style={panelStyle}>
      <div ref={contentRef} className="note-panel flex w-full flex-col rounded-xl border p-2">
        {/* 标题栏（可拖动）+ 关闭 */}
        <div
          data-tauri-drag-region
          className="mb-1 flex shrink-0 select-none items-center justify-between pl-1"
        >
          <span className="note-label text-[0.625em] font-semibold uppercase tracking-[0.14em]">
            便签设置
          </span>
          <button
            type="button"
            aria-label="关闭"
            onClick={onClose}
            className="note-icon-btn note-focus rounded-full p-1 focus-visible:outline-none"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              className="h-[0.75em] w-[0.75em]"
            >
              <path d="M6 6l12 12M18 6 6 18" />
            </svg>
          </button>
        </div>

        {/* ---- 标题 ---- */}
        <div className="px-2 pb-1">
          <div className="note-label text-[0.625em] font-semibold uppercase tracking-[0.14em]">
            标题
          </div>
        </div>
        <div className="px-2 pb-1.5">
          <input
            type="text"
            value={title}
            maxLength={MAX_TITLE_CHARS}
            aria-label="便签标题"
            spellCheck={false}
            onChange={(e) => onTitleInput(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                commitTitle();
                (e.currentTarget as HTMLInputElement).blur();
              }
            }}
            className="note-title-input w-full text-[0.6875em] font-medium outline-none"
          />
        </div>

        {/* ---- 配色 ---- */}
        <div className="px-2 pb-1">
          <div className="note-label text-[0.625em] font-semibold uppercase tracking-[0.14em]">
            配色
          </div>
        </div>
        <div className="flex items-center justify-between px-2 pb-1.5">
          {NOTE_COLORS.map((color) => {
            const selected = color.id === style.color;
            return (
              <button
                key={color.id}
                type="button"
                aria-label={color.label}
                aria-pressed={selected}
                onClick={() => selectColor(color.id)}
                style={{
                  ...swatchStyle(color.id, style.theme),
                  boxShadow: selected
                    ? `0 0 0 2px rgb(${palette.surface}), 0 0 0 3.5px rgb(${palette.accent})`
                    : undefined,
                }}
                className={`note-focus h-[1.25em] w-[1.25em] rounded-full border transition-transform focus-visible:outline-none ${
                  selected ? "" : "hover:scale-110"
                }`}
              />
            );
          })}
        </div>

        {/* ---- 字号 / 主题 ---- */}
        <div className="flex items-center justify-between gap-2 px-2 py-1">
          <span className="note-muted text-[0.6875em]">字号</span>
          <Segmented
            value={style.fontSize}
            ariaLabel="字号"
            options={FONT_SIZES.map((f) => ({ id: f.id, label: f.label }))}
            onChange={(fontSize) => onStyleChange({ fontSize })}
          />
        </div>
        <div className="flex items-center justify-between gap-2 px-2 py-1">
          <span className="note-muted text-[0.6875em]">主题</span>
          <Segmented
            value={style.theme}
            ariaLabel="主题"
            options={THEME_OPTIONS}
            onChange={(theme) => onStyleChange({ theme })}
          />
        </div>

        {/* ---- 透明度 ---- */}
        <div className="px-2 pb-1 pt-0.5">
          <div className="flex items-center justify-between">
            <span className="note-muted text-[0.6875em]">透明度</span>
            <span className="note-faint text-[0.625em] tabular-nums">
              {Math.round(style.opacity * 100)}%
            </span>
          </div>
          <input
            type="range"
            min={Math.round(MIN_OPACITY * 100)}
            max={Math.round(MAX_OPACITY * 100)}
            step={1}
            value={Math.round(style.opacity * 100)}
            aria-label="便签透明度"
            onChange={(e) => onStyleChange({ opacity: Number(e.target.value) / 100 })}
            className="note-range mt-1 w-full"
          />
        </div>

        <div className="note-divider my-1 h-px" />

        <button
          type="button"
          onClick={onHide}
          className="note-muted note-hover note-focus flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[0.75em] focus-visible:outline-none"
        >
          <TrayIcon className="note-faint h-[0.875em] w-[0.875em]" />
          隐藏到托盘
        </button>

        {/* ---- 删除此便签（二次确认，关窗不等于删数据） ---- */}
        {confirmDelete ? (
          <div className="note-subtle-bg mt-1 rounded-lg px-2 py-1.5">
            <div className="note-text text-[0.6875em] leading-snug">
              删除后无法恢复，确认删除这张便签？
            </div>
            <div className="mt-1.5 flex justify-end gap-1.5">
              <button
                type="button"
                onClick={() => onConfirmDeleteChange(false)}
                className="note-muted note-hover-soft rounded-md px-2 py-[3px] text-[0.6875em] transition-colors"
              >
                取消
              </button>
              <button
                type="button"
                onClick={onDelete}
                className="note-danger-outline rounded-md border px-2 py-[3px] text-[0.6875em] transition-colors"
              >
                删除
              </button>
            </div>
          </div>
        ) : (
          <button
            type="button"
            onClick={() => onConfirmDeleteChange(true)}
            className="note-danger note-hover-soft note-focus flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[0.75em] focus-visible:outline-none"
          >
            <TrashIcon className="h-[0.875em] w-[0.875em]" />
            删除此便签
          </button>
        )}

        <div className="note-subtle-bg mt-1 rounded-md px-2 py-1.5">
          <div className="note-label text-[0.5625em] uppercase tracking-[0.12em]">
            数据文件
          </div>
          <div className="note-muted select-text break-all text-[0.625em] leading-snug">
            {storePath || "…"}
          </div>
        </div>
      </div>
    </div>
  );
}
