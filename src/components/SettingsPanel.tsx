import { PowerIcon, TrayIcon } from "./icons";

type Props = {
  pinned: boolean;
  autostart: boolean;
  storePath: string;
  onTogglePin: () => void;
  onToggleAutostart: () => void;
  onHide: () => void;
  onQuit: () => void;
  onClose: () => void;
};

function Switch({ checked }: { checked: boolean }) {
  return (
    <span
      className={`relative h-[18px] w-8 shrink-0 rounded-full transition-colors ${
        checked ? "bg-amber-500" : "bg-stone-300"
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
      className="flex cursor-pointer select-none items-center gap-2 rounded-lg px-2 py-1.5 outline-none transition-colors hover:bg-amber-900/[0.05] focus-visible:bg-amber-900/[0.07]"
    >
      <div className="min-w-0 flex-1">
        <div className="text-[12px] font-medium text-stone-800">{label}</div>
        <div className="text-[10px] leading-snug text-stone-500/75">{desc}</div>
      </div>
      <Switch checked={checked} />
    </div>
  );
}

export default function SettingsPanel({
  pinned,
  autostart,
  storePath,
  onTogglePin,
  onToggleAutostart,
  onHide,
  onQuit,
  onClose,
}: Props) {
  return (
    <>
      {/* 点击面板外关闭 */}
      <div className="fixed inset-0 z-10" onMouseDown={onClose} />

      <div className="fixed right-3 top-[46px] z-20 w-[252px] rounded-xl border border-amber-900/15 bg-[#FFFDF3] p-1.5 shadow-[0_12px_28px_-8px_rgba(40,28,0,0.45)]">
        <ToggleRow
          label="窗口置顶"
          desc="关闭后会被前台应用正常覆盖"
          checked={pinned}
          onChange={onTogglePin}
        />
        <ToggleRow
          label="开机自启"
          desc="登录 Windows 后自动运行"
          checked={autostart}
          onChange={onToggleAutostart}
        />

        <div className="my-1 h-px bg-amber-900/10" />

        <button
          type="button"
          onClick={onHide}
          className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[12px] text-stone-700 transition-colors hover:bg-amber-900/[0.06]"
        >
          <TrayIcon className="h-3.5 w-3.5 text-amber-900/55" />
          隐藏到托盘
        </button>
        <button
          type="button"
          onClick={onQuit}
          className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[12px] text-stone-700 transition-colors hover:bg-red-500/10 hover:text-red-700"
        >
          <PowerIcon className="h-3.5 w-3.5 text-amber-900/55" />
          退出应用
        </button>

        <div className="mt-1 rounded-md bg-amber-900/[0.04] px-2 py-1.5">
          <div className="text-[9px] uppercase tracking-[0.12em] text-amber-900/40">
            数据文件
          </div>
          <div className="select-text break-all text-[10px] leading-snug text-stone-600/80">
            {storePath || "…"}
          </div>
        </div>
      </div>
    </>
  );
}
