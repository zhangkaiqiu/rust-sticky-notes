import { useEffect, useState } from "react";
import { GearIcon, PinIcon } from "./icons";

type Props = {
  pinned: boolean;
  savedTick: number;
  settingsOpen: boolean;
  onTogglePin: () => void;
  onToggleSettings: () => void;
};

/** 顶部条既是拖动区，也放图钉 / 设置按钮 */
export default function TopBar({
  pinned,
  savedTick,
  settingsOpen,
  onTogglePin,
  onToggleSettings,
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
      className="flex h-9 shrink-0 select-none items-center gap-1 pl-3.5 pr-1.5"
    >
      <span
        data-tauri-drag-region
        className="pointer-events-none text-[10px] font-semibold uppercase tracking-[0.16em] text-amber-900/45"
      >
        便签
      </span>

      {/* 剩余空白都可拖动：pointer-events-none 让事件落到父级拖动区 */}
      <div data-tauri-drag-region className="pointer-events-none h-full flex-1" />

      <span
        className={`mr-0.5 text-[10px] text-amber-900/40 transition-opacity duration-300 ${
          justSaved ? "opacity-100" : "opacity-0"
        }`}
      >
        已保存
      </span>

      {/*
        图钉只有两种视觉状态，不再画外圈 / 底色块：
        - 未置顶：竖直、淡色，hover 变深；
        - 置顶：整体旋转 45°（斜着的图钉），直接保持深色。
        旋转加在 svg 自身上（transform-origin 居中），按钮 padding 留白足够、
        且没有 overflow-hidden，所以旋转后不会被裁切，点击热区也不变。
      */}
      <button
        type="button"
        title={pinned ? "取消置顶" : "置顶"}
        aria-pressed={pinned}
        onClick={onTogglePin}
        className="group flex items-center justify-center rounded-full p-[7px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-amber-900/25"
      >
        <PinIcon
          className={`h-[15px] w-[15px] overflow-visible transition-[transform,color] duration-200 ease-out ${
            pinned
              ? "rotate-45 text-amber-900/80"
              : "text-amber-900/55 group-hover:text-amber-900/80"
          }`}
        />
      </button>

      <button
        type="button"
        title="设置"
        aria-expanded={settingsOpen}
        onClick={onToggleSettings}
        className={`rounded-full p-1.5 transition-colors ${
          settingsOpen
            ? "bg-amber-900/12 text-amber-900/85"
            : "text-amber-900/55 hover:bg-amber-900/10 hover:text-amber-900/80"
        }`}
      >
        <GearIcon className="h-[15px] w-[15px]" />
      </button>
    </div>
  );
}
