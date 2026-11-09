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

      <button
        type="button"
        title={pinned ? "取消置顶（会随前台应用被覆盖）" : "置顶（始终显示在最前）"}
        aria-pressed={pinned}
        onClick={onTogglePin}
        className={`rounded-full p-1.5 transition-colors ${
          pinned
            ? "bg-amber-500/85 text-white shadow-sm"
            : "text-amber-900/55 hover:bg-amber-900/10 hover:text-amber-900/80"
        }`}
      >
        <PinIcon className="h-[15px] w-[15px]" />
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
