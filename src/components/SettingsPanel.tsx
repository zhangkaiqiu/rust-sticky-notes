import type { CSSProperties } from "react";
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
 * 便签设置面板：只放**这张便签**的东西（外观 / 置顶 / 隐藏 / 删除）。
 *
 * 「开机自启」「退出应用」这类**整个应用**的操作不在这里——它们属于托盘菜单。
 * 这样两张便签的菜单互不牵连，托盘才管全局。
 */
type Props = {
  pinned: boolean;
  storePath: string;
  style: NoteStyle;
  onStyleChange: (patch: Partial<NoteStyle>) => void;
  onTogglePin: () => void;
  onHide: () => void;
  onDelete: () => void;
  onClose: () => void;
  /** 删除二次确认的展开状态：由 App 持有，右键菜单的「删除此便签」也能直接展开它 */
  confirmDelete: boolean;
  onConfirmDeleteChange: (next: boolean) => void;
};

function Switch({ checked }: { checked: boolean }) {
  return (
    <span
      className={`relative h-[18px] w-8 shrink-0 rounded-full transition-colors ${
        checked ? "note-accent-bg" : "note-switch-off"
      }`}
    >
      <span
        className={`absolute top-[2px] h-[14px] w-[14px] rounded-full bg-white shadow-sm transition-all ${
          checked ? "left-[16px]" : "left-[2px]"
        }`}
      />
    </span>
  );
}

function ToggleRow({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc: string;
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <div
      role="switch"
      aria-checked={checked}
      tabIndex={0}
      onClick={onChange}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onChange();
        }
      }}
      className="note-focus note-hover-soft flex cursor-pointer select-none items-center gap-2 rounded-lg px-2 py-1.5 outline-none focus-visible:outline-none"
    >
      <div className="min-w-0 flex-1">
        <div className="note-text text-[0.75em] font-medium">{label}</div>
        <div className="note-muted-soft text-[0.625em] leading-snug">{desc}</div>
      </div>
      <Switch checked={checked} />
    </div>
  );
}

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
              selected
                ? "note-accent-bg shadow-sm"
                : "note-muted note-hover-soft"
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
  pinned,
  storePath,
  style,
  onStyleChange,
  onTogglePin,
  onHide,
  onDelete,
  onClose,
  confirmDelete,
  onConfirmDeleteChange,
}: Props) {
  const palette = PALETTES[style.color][style.theme];
  // 便签本身可以很透，但设置面板必须保持可读，最低按 90% 不透明度显示
  const panelStyle = {
    "--note-panel-alpha": Math.max(style.opacity, 0.9).toFixed(2),
  } as CSSProperties;

  const selectColor = (color: NoteColor) => onStyleChange({ color });

  return (
    <>
      {/* 点击面板外关闭 */}
      <div className="fixed inset-0 z-10" onMouseDown={onClose} />

      <div
        style={panelStyle}
        className="note-panel note-scroll fixed right-3 top-[46px] z-20 max-h-[calc(100%-58px)] w-[15.75em] overflow-y-auto rounded-xl border p-1.5 shadow-[0_12px_28px_-8px_rgba(40,28,0,0.45)]"
      >
        {/* ---- 配色 ---- */}
        <div className="px-2 pb-1 pt-1">
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
                title={color.label}
                aria-label={color.label}
                aria-pressed={selected}
                onClick={() => selectColor(color.id)}
                style={{
                  ...swatchStyle(color.id, style.theme),
                  boxShadow: selected
                    ? `0 0 0 2px rgb(${palette.panel}), 0 0 0 3.5px rgb(${palette.accent})`
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
            // 1% 一跳（方向键同样按 step 走），百分比数字随 value 同步
            step={1}
            value={Math.round(style.opacity * 100)}
            aria-label="便签透明度"
            onChange={(e) => onStyleChange({ opacity: Number(e.target.value) / 100 })}
            className="note-range mt-1 w-full"
          />
        </div>

        <div className="note-divider my-1 h-px" />

        <ToggleRow
          label="窗口置顶"
          desc="关闭后会被前台应用正常覆盖"
          checked={pinned}
          onChange={onTogglePin}
        />

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
    </>
  );
}
