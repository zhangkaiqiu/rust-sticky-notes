import { useEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  RefObject,
} from "react";
import { KEY_STEP, SPLITTER_HEIGHT_PX, clampRatio } from "../split";

/**
 * 正文区（自由输入）与待办区之间的分割线。
 *
 * 交互：按住这条线上下拖动，实时改变两个区域的高度；双击回到"按内容自适应"。
 *
 * 四条实现约束（都是这类组件的常见坑）：
 * 1. 位置用**绝对坐标**算，不做 `+delta` 累加——鼠标跑快跑慢都不会漂移；
 * 2. 指针捕获在分割线自身上：拖到输入框 / 待办行上方也照样收 move 事件，不会半路丢；
 * 3. 拖动过程只调用 `onChange`（前端 state），松手才 `onCommit`（落盘），
 *    否则每帧一次 IPC + 一次写文件；
 * 4. **不 `preventDefault()`**：那是为了让浏览器不起框选，但取消 pointerdown 的默认行为
 *    会连带把 mousedown / dblclick 一起压掉，双击复位就失灵了。改为同步锁 `user-select`
 *    —— 既挡住了框选，又保住了双击。
 */

type Props = {
  /** 待办区占比；0 = 用户没调过（按内容自适应） */
  ratio: number;
  /** 正文区容器：以它的高度为基准换算比例，窗口缩放时自动跟随 */
  bodyRef: RefObject<HTMLDivElement>;
  /** 拖动 / 方向键过程中实时上报（只改本地显示，不落盘） */
  onChange: (ratio: number) => void;
  /** 松手 / 方向键结束后上报一次（落盘） */
  onCommit: (ratio: number) => void;
};

export default function Splitter({ ratio, bodyRef, onChange, onCommit }: Props) {
  const handleRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{
    pointerId: number;
    /** 按下点相对分割线**底边**（= 待办区顶边）的偏移，保证拖动 1:1、不跳一下 */
    offset: number;
    /** 两区可分配的净高度（已扣掉分割线本身），拖动期间不变 */
    available: number;
    /** 正文 + 待办这一整块的底边在视口中的 y */
    bottom: number;
  } | null>(null);
  /** 本次拖动的最新比例：松手时用它落盘，不依赖 React 状态的更新时机 */
  const latestRef = useRef(0);
  const [dragging, setDragging] = useState(false);
  /** 拖动期间挂上去的全局样式，松手 / 卸载时按记录还原，不留副作用 */
  const styleLockRef = useRef<{ cursor: string; userSelect: string } | null>(null);

  const lockGlobalStyle = () => {
    if (styleLockRef.current) return;
    const root = document.documentElement.style;
    styleLockRef.current = { cursor: root.cursor, userSelect: root.userSelect };
    // 拖过输入框时光标不能变成 I 形；user-select 锁住是为了不把正文一起框选上
    root.cursor = "row-resize";
    root.userSelect = "none";
  };

  const unlockGlobalStyle = () => {
    const saved = styleLockRef.current;
    if (!saved) return;
    styleLockRef.current = null;
    const root = document.documentElement.style;
    root.cursor = saved.cursor;
    root.userSelect = saved.userSelect;
  };

  // 组件被卸载（折叠成条 / 窗口关闭）时若还在拖，也必须还原全局样式
  useEffect(() => unlockGlobalStyle, []);

  /** 结束拖动并落盘。`commit=false` 用于"捕获被系统收走"这种异常收尾 */
  const finish = (commit: boolean) => {
    dragRef.current = null;
    unlockGlobalStyle();
    setDragging(false);
    if (commit && latestRef.current > 0) onCommit(latestRef.current);
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const handle = handleRef.current;
    const body = bodyRef.current;
    if (!handle || !body) return;
    const bodyRect = body.getBoundingClientRect();
    const available = bodyRect.height - SPLITTER_HEIGHT_PX;
    if (!(available > 0)) return;
    dragRef.current = {
      pointerId: event.pointerId,
      offset: event.clientY - handle.getBoundingClientRect().bottom,
      available,
      bottom: bodyRect.bottom,
    };
    latestRef.current = 0;
    handle.setPointerCapture(event.pointerId);
    lockGlobalStyle();
    setDragging(true);
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    const todoTop = event.clientY - drag.offset;
    // 待办区高度 ÷ 净高度 = 用户拖出来的比例
    const next = clampRatio((drag.bottom - todoTop) / drag.available, drag.available);
    latestRef.current = next;
    onChange(next);
  };

  const onPointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    handleRef.current?.releasePointerCapture(event.pointerId);
    // 只点了一下、没有实际移动（latest = 0）就不落盘，保留原来的自适应
    finish(true);
  };

  /** 方向键：↑ 把分割线往上推（待办区变大），↓ 反之；Shift 加速 */
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const body = bodyRef.current;
    const handle = handleRef.current;
    if (!body || !handle) return;
    const available = body.getBoundingClientRect().height - SPLITTER_HEIGHT_PX;
    if (!(available > 0)) return;
    // 起点取**当前真实高度**而不是 ratio：还没调过时也能接着当前位置微调
    const todoEl = handle.nextElementSibling as HTMLElement | null;
    const current = todoEl ? todoEl.getBoundingClientRect().height / available : ratio;
    const step = KEY_STEP * (event.shiftKey ? 3 : 1);
    const next = clampRatio(current + (event.key === "ArrowUp" ? step : -step), available);
    onChange(next);
    onCommit(next);
  };

  return (
    <div
      ref={handleRef}
      role="separator"
      aria-orientation="horizontal"
      aria-label="拖动调整正文与待办区域的高度，双击恢复自适应"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={ratio > 0 ? Math.round(ratio * 100) : undefined}
      tabIndex={0}
      data-dragging={dragging ? "true" : "false"}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => finish(true)}
      // 兜底：捕获被浏览器单方面收走（元素被卸载 / 拖成原生拖放）时也必须收尾
      onLostPointerCapture={() => {
        if (dragRef.current) finish(true);
      }}
      onKeyDown={onKeyDown}
      onDoubleClick={() => {
        // 双击 = 恢复出厂：回到"按内容自适应"
        onChange(0);
        onCommit(0);
      }}
      // 右键不弹菜单：这条线的唯一动作就是拖（例外标记见 App 的右键规则）
      data-no-menu
      className="note-splitter note-focus mx-3 flex shrink-0 items-center outline-none"
    >
      <span className="note-splitter-line w-full" />
    </div>
  );
}
