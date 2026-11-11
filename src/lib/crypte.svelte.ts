// crypte.svelte.ts — état réactif du shell (onglets, peur, powerline,
// overlays, toasts) + branchement minimal aux events backend.
//
// Contrats : commands.md (noms de commandes), contracts.ts (types).
// Les écrans possèdent leurs données ; le shell ne garde que l'état
// global (TUI : app/ui.rs header + status + overlays d'effets).

import type {
  ApparitionState,
  CrypteEvent,
  CrypteUiEvent,
  PendingQuestion,
} from "./contracts";
import {
  CRYPTE_EVENT,
  CRYPTE_UI,
  errText,
  invokeCmd,
  listenCrypte,
  type Unlisten,
} from "./tauri";

/** Onglets du shell : les 5 écrans du TUI (Screen, app/ui.rs). */
export interface ScreenTab {
  id: string;
  route: string;
  label: string;
  /** Touche affichée (TUI : F6 shop, F9 haunt ; settings/vps sans). */
  key: string;
}

export const TABS: ScreenTab[] = [
  { id: "chat", route: "/", label: "chat", key: "" },
  { id: "settings", route: "/settings", label: "settings", key: "" },
  { id: "vps", route: "/vps", label: "vps", key: "" },
  { id: "shop", route: "/shop", label: "shop", key: "F6" },
  { id: "haunt", route: "/haunt", label: "haunt", key: "F9" },
  { id: "mbox", route: "/mbox", label: "mbox", key: "" },
  { id: "bestiary", route: "/bestiary", label: "bestiary", key: "" },
];

/** Durée d'un jump scare : 2 s (apparition.rs SCARE). */
export const SCARE_MS = 2000;

export type ToastKind = "info" | "error" | "success";

export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

let nextToastId = 1;

export class ShellState {
  // Statut powerline (1-based à l'affichage, comme events.rs/sidebar).
  chatIndex = $state(0);
  chatCount = $state(0);
  model = $state("");
  totalTokens = $state(0);
  grandTotal = $state(0);
  tps = $state(0);
  asking = $state(false);

  // Jauge de peur (ctx_gauge : used/window, 0-based interne).
  fearUsed = $state(0);
  fearWindow = $state(1);

  // Écran jeu (hints powerline adaptés, cf. app/haunt.rs HINTS).
  game = $state(false);

  toasts: Toast[] = $state([]);
  question: PendingQuestion | null = $state(null);
  connectOpen = $state(false);
  helpOpen = $state(false);
  jumpscare: ApparitionState | null = $state(null);

  private questionResolve: ((text: string | null) => void) | null = null;
  private scareTimer: number | null = null;

  toast(kind: ToastKind, text: string): void {
    const id = nextToastId++;
    this.toasts.push({ id, kind, text });
    const ttl = kind === "error" ? 9000 : 6000;
    window.setTimeout(() => this.dismissToast(id), ttl);
  }

  dismissToast(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  /** Ouvre la question en modale ; résout avec le texte ou null (Esc). */
  askQuestion(q: PendingQuestion): Promise<string | null> {
    this.dismissQuestion();
    this.question = q;
    return new Promise((resolve) => {
      this.questionResolve = resolve;
    });
  }

  answerQuestion(text: string | null): void {
    this.question = null;
    this.questionResolve?.(text);
    this.questionResolve = null;
  }

  dismissQuestion(): void {
    this.answerQuestion(null);
  }

  /** F5 : créature + éclair 2 s (spook → ApparitionState). */
  async spook(): Promise<void> {
    try {
      const state = await invokeCmd<ApparitionState>("spook");
      this.showScare(state);
    } catch (err) {
      this.toast("error", errText(err));
    }
  }

  showScare(state: ApparitionState): void {
    this.jumpscare = state;
    if (this.scareTimer !== null) window.clearTimeout(this.scareTimer);
    this.scareTimer = window.setTimeout(() => {
      this.jumpscare = null;
      this.scareTimer = null;
    }, SCARE_MS);
  }

  /** Branche les events globaux du shell (erreurs → toasts, scare). */
  async startShellListeners(): Promise<Unlisten> {
    const offEvent = await listenCrypte(CRYPTE_EVENT, (e: CrypteEvent) =>
      this.onDomainEvent(e),
    );
    const offUi = await listenCrypte(CRYPTE_UI, (e: CrypteUiEvent) =>
      this.onUiEvent(e),
    );
    return () => {
      offEvent();
      offUi();
    };
  }

  private onDomainEvent(e: CrypteEvent): void {
    if (e.type === "assistant_error") {
      this.toast("error", `chat ${e.chat} : ${e.error}`);
    } else if (e.type === "agent_failed") {
      this.toast("error", `agent #${e.agent} : ${e.error}`);
    } else if (e.type === "mailbox" && e.priorite === "P0") {
      this.toast("error", `[${e.canal}] ${e.texte}`);
    }
  }

  private onUiEvent(e: CrypteUiEvent): void {
    if (e.kind === "apparition" && e.state.omen === "scare") {
      this.showScare(e.state);
    }
  }
}

export const shell = new ShellState();
