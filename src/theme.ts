import type { CSSProperties } from "react";

/**
 * 便签外观：配色 / 字号 / 深浅主题 / 透明度。
 *
 * 颜色统一用 "r g b" 三个通道的字符串表示，便于在 CSS 里用
 * `rgb(var(--x) / alpha)` 组合出任意透明度；透明度只作用于卡片表面
 * （底色 + 边框），文字与图标始终使用各自固定的 alpha，低透明度下依然清晰。
 */

export type NoteColor = "cream" | "mint" | "rose" | "sky" | "sand" | "graphite";
export type NoteTheme = "light" | "dark";
export type FontSize = "sm" | "md" | "lg";

export type NoteStyle = {
  color: NoteColor;
  theme: NoteTheme;
  fontSize: FontSize;
  /** 卡片表面透明度，0.35–1 */
  opacity: number;
};

export const NOTE_COLORS: { id: NoteColor; label: string }[] = [
  { id: "cream", label: "奶油" },
  { id: "mint", label: "薄荷" },
  { id: "rose", label: "樱粉" },
  { id: "sky", label: "天蓝" },
  { id: "sand", label: "沙金" },
  { id: "graphite", label: "石墨" },
];

export const FONT_SIZES: { id: FontSize; label: string; px: number }[] = [
  { id: "sm", label: "小", px: 14 },
  { id: "md", label: "中", px: 16 },
  { id: "lg", label: "大", px: 18 },
];

export const MIN_OPACITY = 0.35;
export const MAX_OPACITY = 1;

export const DEFAULT_STYLE: NoteStyle = {
  color: "cream",
  theme: "light",
  fontSize: "md",
  opacity: 0.97,
};

/** 每套配色在浅色 / 深色下各一套色板（深色不是简单反色，单独降饱和） */
type Palette = {
  surface: string;
  border: string;
  text: string;
  muted: string;
  icon: string;
  accent: string;
  onAccent: string;
  panel: string;
  danger: string;
  switchOff: string;
};

export const PALETTES: Record<NoteColor, Record<NoteTheme, Palette>> = {
  cream: {
    light: {
      surface: "255 248 214",
      border: "120 53 15",
      text: "41 37 36",
      muted: "120 113 108",
      icon: "120 53 15",
      accent: "245 158 11",
      onAccent: "255 255 255",
      panel: "255 253 243",
      danger: "220 38 38",
      switchOff: "214 211 209",
    },
    dark: {
      surface: "42 38 34",
      border: "174 148 104",
      text: "243 235 221",
      muted: "185 172 152",
      icon: "216 194 156",
      accent: "224 166 60",
      onAccent: "26 21 14",
      panel: "51 46 40",
      danger: "248 113 113",
      switchOff: "87 83 78",
    },
  },
  mint: {
    light: {
      surface: "228 243 231",
      border: "6 78 59",
      text: "28 58 44",
      muted: "91 122 106",
      icon: "16 101 74",
      accent: "52 168 124",
      onAccent: "255 255 255",
      panel: "242 250 244",
      danger: "220 38 38",
      switchOff: "209 213 219",
    },
    dark: {
      surface: "34 48 41",
      border: "127 185 159",
      text: "232 244 236",
      muted: "170 198 184",
      icon: "154 211 185",
      accent: "77 190 146",
      onAccent: "16 28 22",
      panel: "42 58 50",
      danger: "248 113 113",
      switchOff: "75 85 99",
    },
  },
  rose: {
    light: {
      surface: "251 230 233",
      border: "131 24 67",
      text: "61 31 42",
      muted: "138 91 106",
      icon: "159 18 57",
      accent: "228 99 138",
      onAccent: "255 255 255",
      panel: "253 242 244",
      danger: "220 38 38",
      switchOff: "214 211 209",
    },
    dark: {
      surface: "51 35 42",
      border: "192 142 158",
      text: "247 231 235",
      muted: "201 169 179",
      icon: "226 165 183",
      accent: "233 122 158",
      onAccent: "32 18 23",
      panel: "59 42 50",
      danger: "248 113 113",
      switchOff: "87 83 78",
    },
  },
  sky: {
    light: {
      surface: "230 240 251",
      border: "30 58 138",
      text: "27 43 64",
      muted: "91 110 133",
      icon: "29 78 137",
      accent: "59 130 196",
      onAccent: "255 255 255",
      panel: "243 248 254",
      danger: "220 38 38",
      switchOff: "214 211 209",
    },
    dark: {
      surface: "32 43 56",
      border: "129 158 190",
      text: "230 239 249",
      muted: "169 189 211",
      icon: "158 196 232",
      accent: "90 169 230",
      onAccent: "16 26 36",
      panel: "39 50 63",
      danger: "248 113 113",
      switchOff: "75 85 99",
    },
  },
  sand: {
    light: {
      surface: "245 233 207",
      border: "120 87 30",
      text: "59 50 32",
      muted: "124 108 76",
      icon: "138 106 36",
      accent: "201 154 46",
      onAccent: "255 255 255",
      panel: "251 244 227",
      danger: "220 38 38",
      switchOff: "214 211 209",
    },
    dark: {
      surface: "50 44 34",
      border: "189 163 118",
      text: "243 234 216",
      muted: "198 183 156",
      icon: "217 191 140",
      accent: "216 172 76",
      onAccent: "28 23 15",
      panel: "58 51 40",
      danger: "248 113 113",
      switchOff: "87 83 78",
    },
  },
  graphite: {
    light: {
      surface: "236 236 237",
      border: "82 82 91",
      text: "39 39 42",
      muted: "98 98 107",
      icon: "63 63 70",
      accent: "113 113 122",
      onAccent: "255 255 255",
      panel: "246 246 247",
      danger: "220 38 38",
      switchOff: "212 212 216",
    },
    dark: {
      surface: "38 38 42",
      border: "142 142 152",
      text: "236 236 240",
      muted: "173 173 182",
      icon: "198 198 206",
      accent: "156 163 175",
      onAccent: "24 24 27",
      panel: "46 46 51",
      danger: "248 113 113",
      switchOff: "82 82 91",
    },
  },
};

export function clampOpacity(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_STYLE.opacity;
  return Math.min(MAX_OPACITY, Math.max(MIN_OPACITY, value));
}

/** 前端也做一次归一化，防止旧数据 / 手写 JSON 带进非法值 */
export function normalizeStyle(input: Partial<NoteStyle> | null | undefined): NoteStyle {
  const color = NOTE_COLORS.some((c) => c.id === input?.color)
    ? (input!.color as NoteColor)
    : DEFAULT_STYLE.color;
  const theme = input?.theme === "dark" ? "dark" : "light";
  const fontSize = FONT_SIZES.some((f) => f.id === input?.fontSize)
    ? (input!.fontSize as FontSize)
    : DEFAULT_STYLE.fontSize;
  return { color, theme, fontSize, opacity: clampOpacity(input?.opacity ?? DEFAULT_STYLE.opacity) };
}

export function fontPx(fontSize: FontSize): number {
  return FONT_SIZES.find((f) => f.id === fontSize)?.px ?? FONT_SIZES[1].px;
}

/** 把配色转成一组 CSS 变量，挂在便签根节点上；子元素只引用变量 */
export function styleVars(style: NoteStyle): CSSProperties {
  const palette = PALETTES[style.color][style.theme];
  return {
    "--note-surface": palette.surface,
    "--note-border": palette.border,
    "--note-text": palette.text,
    "--note-muted": palette.muted,
    "--note-icon": palette.icon,
    "--note-accent": palette.accent,
    "--note-on-accent": palette.onAccent,
    "--note-panel": palette.panel,
    "--note-danger": palette.danger,
    "--note-switch-off": palette.switchOff,
    "--note-alpha": clampOpacity(style.opacity).toFixed(2),
    "--note-font-size": `${fontPx(style.fontSize)}px`,
    colorScheme: style.theme,
  } as CSSProperties;
}

/** 设置面板里的色块预览（用当前主题下的表面色 + 边框色） */
export function swatchStyle(color: NoteColor, theme: NoteTheme): CSSProperties {
  const palette = PALETTES[color][theme];
  return {
    backgroundColor: `rgb(${palette.surface})`,
    borderColor: `rgb(${palette.border} / 0.45)`,
  };
}
