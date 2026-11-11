// Rendu des sprites bestiaire 32x32 (miroir de bestiary.rs).
// Une lettre = une couleur du theme, '.' = transparent. Facteur entier
// uniquement, rendu net (canvas + CSS image-rendering: pixelated).
import { CRYPTE_THEME } from "./contracts";

export const SPRITE = 32;

/** Table lettres -> couleurs, miroir exact de `palette()` bestiary.rs. */
export const LETTER_COLORS: Readonly<Record<string, string>> = {
  W: CRYPTE_THEME.bone,
  S: CRYPTE_THEME.sand,
  U: CRYPTE_THEME.umber,
  K: CRYPTE_THEME.ink,
  O: CRYPTE_THEME.pumpkin,
  o: CRYPTE_THEME.pumpkinDark,
  R: CRYPTE_THEME.blood,
  r: CRYPTE_THEME.dried,
  G: CRYPTE_THEME.ecto,
  g: CRYPTE_THEME.ectoDark,
  P: CRYPTE_THEME.spectre,
  p: CRYPTE_THEME.spectreDark,
  Y: CRYPTE_THEME.candle,
  y: CRYPTE_THEME.candleDark,
  N: CRYPTE_THEME.burlap,
};

/** Couleur hex d'une lettre, `null` pour '.' (transparent) ou inconnue. */
export function colorForLetter(ch: string): string | null {
  if (ch === ".") return null;
  return LETTER_COLORS[ch] ?? null;
}

/** `true` si la grille est un sprite 32x32 valide (lettres connues). */
export function isValidGrid(grid: string[]): boolean {
  if (grid.length !== SPRITE) return false;
  return grid.every(
    (row) =>
      row.length === SPRITE &&
      [...row].every((ch) => ch === "." || ch in LETTER_COLORS),
  );
}

/** Dessine la grille sur le canvas au facteur entier `scale` (>= 1). */
export function drawGrid(
  canvas: HTMLCanvasElement,
  grid: string[],
  scale = 4,
): void {
  const rows = grid.length;
  const cols = grid.length > 0 ? (grid[0]?.length ?? 0) : 0;
  const k = Math.max(1, Math.floor(scale));
  canvas.width = cols * k;
  canvas.height = rows * k;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  for (let y = 0; y < rows; y += 1) {
    const row = grid[y] ?? "";
    for (let x = 0; x < row.length; x += 1) {
      const color = colorForLetter(row[x] ?? ".");
      if (color === null) continue;
      ctx.fillStyle = color;
      ctx.fillRect(x * k, y * k, k, k);
    }
  }
}

/** Facteur entier pour remplir `box` (0 si grille vide). */
export function bestScale(
  grid: string[],
  box: { w: number; h: number },
): number {
  const rows = grid.length;
  const cols = rows > 0 ? (grid[0]?.length ?? 0) : 0;
  if (rows === 0 || cols === 0 || box.w <= 0 || box.h <= 0) return 0;
  return Math.max(1, Math.floor(Math.min(box.w / cols, box.h / rows)));
}
