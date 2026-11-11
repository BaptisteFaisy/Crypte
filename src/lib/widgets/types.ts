// types.ts — types partagés des widgets Crypte (source unique).
// Les tranches shell/chat/screens importent d'ici via le barrel index.ts.

/** Ton d'un badge : une couleur du thème Crypte. */
export type BadgeTone =
  | "pumpkin"
  | "blood"
  | "ecto"
  | "spectre"
  | "candle"
  | "wisp"
  | "ash"
  | "moss";

/** Entrée de menu (CrypteMenu). */
export interface MenuItem {
  id: string;
  label: string;
  hint?: string;
  disabled?: boolean;
  danger?: boolean;
}

/** Séparateur de menu. */
export interface MenuSeparator {
  separator: true;
}

export type MenuEntry = MenuItem | MenuSeparator;

/** Onglet (CrypteTabs). */
export interface WidgetTab {
  id: string;
  label: string;
  badge?: string | number | null;
  disabled?: boolean;
}
