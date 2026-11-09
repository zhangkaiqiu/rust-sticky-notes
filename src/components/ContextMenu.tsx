import { useEffect, useLayoutEffect, useRef, useState } from "react";

/**
 * 便签右键菜单（纯 HTML/CSS，不用原生菜单弹窗）。
 *
 * 无边框透明窗口里用原生菜单会碰到坐标换算 / 被裁切的问题，所以自己画：
 * - 用 `position: fixed` + clientX/clientY 定位；
 * - 贴近窗口右 / 下边缘时自动翻转，保证菜单完整可见；
 * - 点外部、Esc、滚动、窗口失焦 / 尺寸变化都关闭；
 * - 视觉沿用便签的暖色面板变量（note-panel）。
 */

export type ContextMenuItem =
  | { kind: "separator"; id: string }
  | {
      kind?: "item";
      id: string;
      label: string;
      /** 右侧展示的快捷键提示，纯装饰 */
      shortcut?: string;
      danger?: boolean;
      disabled?: boolean;
      onSelect: () => void;
    };

type Props = {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
};

/** 距离窗口边缘的最小留白 */
const EDGE = 6;

export default function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [pos, setPos] = useState({ x, y });

  // 先按原始坐标渲染，量到实际尺寸后再决定是否翻转（只需一次 layout 开销）
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const maxX = window.innerWidth - width - EDGE;
    const maxY = window.innerHeight - height - EDGE;
    setPos({
      x: x > maxX ? Math.max(EDGE, x - width) : x,
      y: y > maxY ? Math.max(EDGE, y - height) : y,
    });
  }, [x, y]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // 捕获阶段拦下：优先关菜单，不要顺带关掉设置面板
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }
    };
    const close = () => onClose();

    window.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", close, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("resize", close);
    };
  }, [onClose]);

  return (
    <>
      {/* 点击 / 右键菜单外的任何地方都关闭 */}
      <div
        className="fixed inset-0 z-30"
        onMouseDown={onClose}
        onContextMenu={(event) => {
          event.preventDefault();
          onClose();
        }}
      />

      <div
        ref={ref}
        role="menu"
        style={{ left: pos.x, top: pos.y }}
        className="note-panel fixed z-40 min-w-[9.75em] select-none rounded-[10px] border p-1 shadow-[0_14px_30px_-10px_rgba(40,28,0,0.55)]"
      >
        {items.map((item) =>
          item.kind === "separator" ? (
            <div key={item.id} className="note-divider mx-1 my-1 h-px" />
          ) : (
            <button
              key={item.id}
              type="button"
              role="menuitem"
              disabled={item.disabled}
              onClick={() => {
                if (item.disabled) return;
                onClose();
                item.onSelect();
              }}
              className={`flex w-full items-center gap-3 rounded-md px-2 py-[5px] text-left text-[0.75em] transition-colors ${
                item.disabled
                  ? "note-faint cursor-default"
                  : item.danger
                    ? "note-danger note-danger-hover"
                    : "note-text note-hover"
              }`}
            >
              <span className="flex-1 truncate">{item.label}</span>
              {item.shortcut && (
                <span className="note-faint text-[0.8125em] tabular-nums">
                  {item.shortcut}
                </span>
              )}
            </button>
          )
        )}
      </div>
    </>
  );
}
