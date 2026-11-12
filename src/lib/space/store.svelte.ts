// store.svelte.ts — état global du fond 3D : bascule F8, vue hub F7,
// écoute du canal `space-event` (ou scénario de démo hors Tauri).
import { invokeCmd, isTauri, listenCrypte, type Unlisten } from "../tauri";
import { startDemo } from "./demo";
import { apply } from "./state";
import {
  newSceneState,
  type ProjectTree,
  type SceneEvent,
  type SceneState,
} from "./types";

/// Canal Tauri des événements de scène (miroir de `SpaceLog` côté Rust).
export const SPACE_EVENT = "space-event";

class SpaceStore {
  /// F8 : le fond 3D est-il actif ?
  enabled = $state(false);
  /// Le monde logique, plié depuis les événements.
  state: SceneState = newSceneState();

  private off: Unlisten | null = null;
  private stopDemo: (() => void) | null = null;
  private t0 = 0;

  /// Horloge de la scène (secondes depuis l'activation).
  now(): number {
    return (performance.now() - this.t0) / 1000;
  }

  async toggle(): Promise<void> {
    if (this.enabled) this.disable();
    else await this.enable();
  }

  async enable(): Promise<void> {
    if (this.enabled) return;
    this.state = newSceneState();
    this.t0 = performance.now();
    if (isTauri()) {
      try {
        await invokeCmd("space_attach");
      } catch {
        // Le backend répondra au prochain enable.
      }
      try {
        const tree = await invokeCmd<ProjectTree>("space_scan", {
          workspaces: [],
        });
        apply(
          this.state,
          { type: "project_open", path: tree.root, tree },
          0,
        );
      } catch {
        // Le module space du backend répondra au prochain enable.
      }
      this.off = await listenCrypte(SPACE_EVENT, (e: SceneEvent) =>
        apply(this.state, e, this.now()),
      );
    } else {
      this.stopDemo = startDemo(this.state, () => this.now());
    }
    this.enabled = true;
  }

  disable(): void {
    if (isTauri()) void invokeCmd("space_detach").catch(() => {});
    this.off?.();
    this.off = null;
    this.stopDemo?.();
    this.stopDemo = null;
    this.enabled = false;
  }

  /// F7 : la caméra s'éloigne sur tous les projets (ou revient).
  toggleHub(): void {
    apply(
      this.state,
      { type: this.state.hub ? "hub_close" : "hub_open" },
      this.now(),
    );
  }
}

export const space = new SpaceStore();
