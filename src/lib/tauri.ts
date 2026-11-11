// tauri.ts — invoke/listen sûrs, avec repli hors Tauri (dev navigateur).
//
// Les args peuvent contenir une clé API (connect_save, provider_set_key)
// ou un secret jeton : ils ne sont JAMAIS loggués ni stockés ici.

import type { CrypteEvent, CrypteUiEvent } from "./contracts";
import { mockFor } from "./fixtures";

export const CRYPTE_EVENT = "crypte-event";
export const CRYPTE_UI = "crypte-ui";

/** Vrai quand le frontend tourne dans le webview Tauri. */
export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in window
  );
}

export type Unlisten = () => void;

/**
 * Appelle une commande Tauri. Hors Tauri (navigateur de dev), des
 * fixtures répondent (jamais de secret, jamais le réseau).
 */
export async function invokeCmd<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!isTauri()) {
    return mockFor(cmd, args ?? {}) as T;
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Écoute un canal backend → frontend. Hors Tauri : noop. */
export async function listenCrypte(
  channel: typeof CRYPTE_EVENT,
  cb: (event: CrypteEvent) => void,
): Promise<Unlisten>;
export async function listenCrypte(
  channel: typeof CRYPTE_UI,
  cb: (event: CrypteUiEvent) => void,
): Promise<Unlisten>;
export async function listenCrypte(
  channel: string,
  cb: (event: never) => void,
): Promise<Unlisten> {
  if (!isTauri()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  return listen<never>(channel, (e) => cb(e.payload));
}

/** Message d'erreur affichable, sans secret (backend déjà caviarde). */
export function errText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "erreur inconnue";
}
