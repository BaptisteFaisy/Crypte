// keys.ts — raccourcis clavier de l'écran Chat (miroir app/keys.rs + scroll).
// Pur et testable : ne touche ni au DOM ni à Tauri.

export type ChatKeyAction =
  | "send"
  | "stop"
  | "new-chat"
  | "close-chat"
  | "spook"
  | "scroll-up"
  | "scroll-down"
  | "scroll-bottom"
  | "cancel-flow";

/** Lignes remontées par cran (molette / Shift+flèches / PgUp/PgDn). */
export const SCROLL_LINE = 3;
export const SCROLL_PAGE = 12;

/**
 * Action d'une frappe clavier dans la zone de saisie ou la page chat.
 * `inInput` : le focus est dans la saisie (Entrée = envoi).
 */
export function keyAction(e: {
  key: string;
  ctrl: boolean;
  shift: boolean;
  inInput: boolean;
}): ChatKeyAction | null {
  if (e.ctrl && (e.key === "c" || e.key === "C")) return "stop";
  if (e.ctrl && (e.key === "n" || e.key === "N")) return "new-chat";
  if (e.ctrl && (e.key === "w" || e.key === "W")) return "close-chat";
  if (e.key === "F5") return "spook";
  if (e.key === "Escape") return "cancel-flow";
  if (e.key === "End") return "scroll-bottom";
  if (e.key === "PageUp") return "scroll-up";
  if (e.key === "PageDown") return "scroll-down";
  if (e.shift && e.key === "ArrowUp") return "scroll-up";
  if (e.shift && e.key === "ArrowDown") return "scroll-down";
  if (e.inInput && e.key === "Enter" && !e.shift) return "send";
  return null;
}

/** Nouveau scrollTop après un cran (jamais hors bornes). */
export function stepScroll(
  top: number,
  max: number,
  action: "scroll-up" | "scroll-down",
  page: boolean,
): number {
  const step = page ? SCROLL_PAGE * 16 : SCROLL_LINE * 16;
  const next = action === "scroll-up" ? top - step : top + step;
  return Math.min(Math.max(0, max), Math.max(0, next));
}

/** Lignes remontées depuis le bas (badge `↕ N lines ↓`). */
export function linesUp(top: number, max: number, linePx: number): number {
  if (linePx <= 0) return 0;
  return Math.max(0, Math.round((max - top) / linePx));
}
