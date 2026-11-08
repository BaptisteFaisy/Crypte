// Miroir des tests bestiary.rs : la table lettres->couleurs suit le theme.
import { CRYPTE_THEME } from "./contracts";
import { describe, expect, it } from "vitest";
import { colorForLetter, isValidGrid, LETTER_COLORS, SPRITE } from "./pixel";

function blankGrid(): string[] {
  return Array.from({ length: SPRITE }, () => ".".repeat(SPRITE));
}

describe("LETTER_COLORS", () => {
  it("couvre les 15 lettres de palette() bestiary.rs", () => {
    expect(Object.keys(LETTER_COLORS).sort().join("")).toBe("GKNOPRSUWYgopry");
  });

  it("pointe chaque lettre vers la bonne couleur du theme", () => {
    const pairs: [string, string][] = [
      ["W", CRYPTE_THEME.bone],
      ["S", CRYPTE_THEME.sand],
      ["U", CRYPTE_THEME.umber],
      ["K", CRYPTE_THEME.ink],
      ["O", CRYPTE_THEME.pumpkin],
      ["o", CRYPTE_THEME.pumpkinDark],
      ["R", CRYPTE_THEME.blood],
      ["r", CRYPTE_THEME.dried],
      ["G", CRYPTE_THEME.ecto],
      ["g", CRYPTE_THEME.ectoDark],
      ["P", CRYPTE_THEME.spectre],
      ["p", CRYPTE_THEME.spectreDark],
      ["Y", CRYPTE_THEME.candle],
      ["y", CRYPTE_THEME.candleDark],
      ["N", CRYPTE_THEME.burlap],
    ];
    for (const [letter, hex] of pairs) expect(LETTER_COLORS[letter]).toBe(hex);
  });
});

describe("colorForLetter", () => {
  it("rend null pour '.' et les inconnues", () => {
    expect(colorForLetter(".")).toBeNull();
    for (const ch of ["Z", "z", " ", "Q"]) expect(colorForLetter(ch)).toBeNull();
  });
});

describe("isValidGrid", () => {
  it("accepte un sprite 32x32 valide", () => {
    expect(isValidGrid(blankGrid())).toBe(true);
  });

  it("refuse les grilles mal formees (miroir decode_rejects_bad_grids)", () => {
    expect(isValidGrid(["..", ".."])).toBe(false);
    expect(isValidGrid(blankGrid().slice(0, 31))).toBe(false);
    const ragged = blankGrid();
    ragged[0] = ".".repeat(31);
    expect(isValidGrid(ragged)).toBe(false);
    const alien = blankGrid();
    alien[5] = ".".repeat(31) + "Z";
    expect(isValidGrid(alien)).toBe(false);
  });
});
