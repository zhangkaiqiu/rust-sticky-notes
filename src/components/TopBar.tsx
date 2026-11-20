import { useEffect, useState } from "react";
import { ChevronIcon } from "./icons";

type Props = {
  title: string;
  collapsed: boolean;
  savedTick: number;
  onToggleCollapsed: () => void;
};

/**
 * 标题栏高度（逻辑像素）。
 *
 * **这是唯一的几何事实源**，两处必须同源，否则一定出 bug：
 * 1. 标题栏自身高度；
 * 2. Rust 侧折叠态窗口高度（`lib.rs` 的 `COLLAPSED_HEIGHT`）。
 *
 * 之前浮动按钮组自己写死 `h-9`(36px)、标题栏是 40px，两者差 4px，
 * 折叠切换时图钉和齿轮就会上下跳——这就是"多源几何"的典型代价。
 * 现在高度一律引用这个常量，谁都不许再写字面量。
 */
export const BAR_HEIGHT = 40;

/**
 * 顶部条：整条都是拖动区，只有真正有交互的元素才拦下鼠标。
 *
 * 关于 `data-tauri-drag-region`（踩过的坑）：
 * 这个属性**不会继承**。Tauri 只认鼠标按下时命中的那个元素有没有该属性，
 * 所以标题、占位块这些子元素必须各自带上，否则它们覆盖的那块区域拖不动——
 * 表现为"标题栏中间一大片是死区"。
 *
 * 判断规则（以后新增元素照此办理）：
 * - 纯显示 / 占位 / 文字  → 带 `data-tauri-drag-region`，让窗口可拖；
 * - 按钮这类有点击行为的 → **不带**，并在 `onMouseDown` 里 stopPropagation，
 *   避免按下就被当成拖动。
 */
export default function TopBar({
  title,
  collapsed,
  savedTick,
  onToggleCollapsed,
}: Props) {
  const [justSaved, setJustSaved] = useState(false);

  useEffect(() => {
    if (savedTick === 0) return;
    setJustSaved(true);
    const t = window.setTimeout(() => setJustSaved(false), 1400);
    return () => window.clearTimeout(t);
  }, [savedTick]);

  return (
    <div
      data-tauri-drag-region
      style={{ height: BAR_HEIGHT }}
      // 整条标题栏都不弹自定义菜单（data-no-menu 由 App 的右键规则识别）。
      // 折叠态只有 40px 高，菜单必然被窗口裁掉；展开态也只和标题有关、没有可操作项。
      data-no-menu
      // 两态用同一套 class：没有任何 collapsed 分支，几何就不可能漂移
      className="flex shrink-0 select-none items-center gap-1 pl-1 pr-[4.5rem]"
    >
      {/*
        折叠 / 展开箭头：唯一的折叠开关。
        mousedown 阻止冒泡，点它只切换折叠，不会误触标题栏拖拽。
      */}
      <button
        type="button"
        aria-label={collapsed ? "展开便签" : "折叠成条"}
        aria-expanded={!collapsed}
        onClick={onToggleCollapsed}
        onMouseDown={(e) => e.stopPropagation()}
        className="note-icon-btn note-focus flex shrink-0 items-center justify-center rounded-full p-2 focus-visible:outline-none"
      >
        <ChevronIcon
          className={`h-[0.875em] w-[0.875em] transition-transform duration-200 ${
            collapsed ? "rotate-180" : ""
          }`}
        />
      </button>

      {/*
        标题：**纯展示 + 拖动**，不响应右键（标题栏整条已是 data-no-menu 区）。
        也不加 title 属性：原生 tooltip 又慢又丑，还把标题重复显示一遍。
      */}
      <span
        data-tauri-drag-region
        className="note-title-label min-w-0 flex-1 truncate text-[0.625em] font-semibold uppercase tracking-[0.14em]"
      >
        {title}
      </span>

      {/* 折叠成条时空间紧张，保存提示让位（保存状态仍照常落盘） */}
      {!collapsed && (
        <span
          data-tauri-drag-region
          className={`note-faint shrink-0 text-[0.625em] transition-opacity duration-300 ${
            justSaved ? "opacity-100" : "opacity-0"
          }`}
        >
          已保存
        </span>
      )}
    </div>
  );
}
