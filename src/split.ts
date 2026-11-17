/**
 * 正文区 / 待办区分割的**唯一**几何事实源。
 *
 * 比例 `ratio = 待办区高度 ÷ (正文区 + 待办区)`，存进 store.json 的 `splitRatio`。
 * 为什么不存像素：窗口被拉大拉小时两个区域要一起缩放，存像素会让待办区顶满整张便签。
 *
 * `0` 是**哨兵值**，表示"用户没调过"：这时前端走改造前的自适应布局
 * （正文区的 flex-1 + 待办区按内容撑开、最多 44%），不是"比例为 0 的布局"。
 */

/** 正文区最少高度（CSS 像素），与 textarea 的 `min-h-[72px]` 同源 */
export const MIN_TEXT_PX = 72;
/** 待办区最少高度（CSS 像素）：标题 + 至少一行 + 输入条 */
export const MIN_TODO_PX = 88;

/** 方向键 / 键盘微调步长（比例），按住 Shift 走 3 倍 */
export const KEY_STEP = 0.02;

/**
 * 分割线自身占的高度（CSS 像素）。
 *
 * 手动模式下两个区域的 flex-basis 必须从 `100%` 里先**扣掉**这 9px，
 * 否则三者相加超过容器高度，flex 会把两边各缩一点——比例就不再是用户拖的那个，
 * 表现为"拖到 50/50 看着不像一半"。样式里 `.note-splitter` 的高度也必须是这个值。
 */
export const SPLITTER_HEIGHT_PX = 9;

/** 供两个区域分配的净高度（扣掉分割线本身） */
const AVAILABLE = `(100% - ${SPLITTER_HEIGHT_PX}px)`;

/**
 * 手动模式下的 flex-basis。
 *
 * 外层套 `clamp()` 而不是只写百分比：窗口被拉矮时**比例不变**，但按比例算出来的
 * 某一区可能已经小于它的像素下限（比如 260px 高的便签里待办占 20% = 45px，
 * 放不下一行待办 + 输入条）。这时由浏览器把这一区兜在 88px，
 * 另一区相应让出空间——JS 的 `clampRatio` 只管拖动过程中的算式，管不到"窗口变小"这条路。
 */
const basisCss = (fraction: number, minPx: number, maxPx: number) => {
  const ratio = Math.min(Math.max(fraction, 0), 1).toFixed(4);
  return `clamp(${minPx}px, ${AVAILABLE} * ${ratio}, ${AVAILABLE} - ${maxPx}px)`;
};

/** 手动模式下正文区的 flex-basis（正文占比 = 1 - ratio，最少 72px、最多「净高 - 待办下限」） */
export function textBasis(ratio: number): string {
  return basisCss(1 - ratio, MIN_TEXT_PX, MIN_TODO_PX);
}

/** 手动模式下待办区的 flex-basis（= ratio，最少 88px、最多「净高 - 正文下限」） */
export function todoBasis(ratio: number): string {
  return basisCss(ratio, MIN_TODO_PX, MIN_TEXT_PX);
}

/**
 * 把比例夹进"两个区域都还看得见"的区间。
 *
 * 上下限按**像素**算（而不是写死 0.18 / 0.82），因为同一个比例在 260px 高的便签里
 * 是 100px 的待办区、在 840px 高的便签里就是 320px——只有像素下限才跟"能不能用"对得上。
 *
 * `availableHeight` 是**扣掉分割线之后**、供两个区域分配的净高度
 * （`bodyHeight - SPLITTER_HEIGHT_PX`）。口径必须和 flex-basis 的分母一致，
 * 否则拖到某个位置会出现"算出来 50%，看着不像一半"。
 */
export function clampRatio(next: number, availableHeight: number): number {
  if (!Number.isFinite(next)) return 0;
  if (!(availableHeight > 0)) return 0;
  // 高度不够放下两个下限时，退化成"对半分"，总比夹出一个空区间稳
  if (availableHeight < MIN_TEXT_PX + MIN_TODO_PX) return 0.5;
  const lo = MIN_TODO_PX / availableHeight;
  const hi = 1 - MIN_TEXT_PX / availableHeight;
  return Math.min(Math.max(next, lo), hi);
}
