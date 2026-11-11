// store.ts — état réactif de l'écran Chat, branché sur les commandes
// et events du PLAN (commands.md + contracts.ts). Zéro duplication de
// types : tout vient de `../plan/contracts` (l'intégrateur rewire vers
// `$lib/contracts`). Conventions : index 0-based côté commandes,
// numéros 1-based dans les events (convertis ici).

import { CRYPTE_EVENT, CRYPTE_UI, invokeCmd, listenCrypte, type Unlisten } from "../tauri";
import { derived, get, writable } from "svelte/store";
import type {
  ApparitionState,
  ChatState,
  ChatSummary,
  CompactResult,
  ConnectStatus,
  Creature,
  CrypteEvent,
  CrypteUiEvent,
  HubSnapshot,
  ProviderInfo,
  SubAgentInfo,
  ToolConfirmRequest,
} from "../contracts";

// ------------------------------------------------------------------ état

export const chats = writable<ChatSummary[]>([]);
export const activeIndex = writable(0);
export const chatState = writable<ChatState | null>(null);
export const agents = writable<SubAgentInfo[]>([]);
export const apparition = writable<ApparitionState | null>(null);
export const spooked = writable(false);
export const critic = writable<string | null>(null);
export const kiroIdle = writable<Creature | null>(null);
export const kiroSpooked = writable<Creature | null>(null);
export const fear = writable(0);
export const flash = writable(0);
export const connectStatus = writable<ConnectStatus | null>(null);
export const providers = writable<ProviderInfo[]>([]);
export const gauge = writable<{ used: number; window: number } | null>(null);
export const pendingConfirm = writable<ToolConfirmRequest | null>(null);
export const hubSnapshot = writable<HubSnapshot | null>(null);
export const notice = writable<string | null>(null);

export interface ConnectFlow {
  provider: string;
  step: "key" | "model";
  key: string;
  buffer: string;
  defaultModel: string;
}
export const connectFlow = writable<ConnectFlow | null>(null);

/** Libellé du prompt de saisie (connect.rs `sync_flow_prompt`). */
export const promptLabel = derived(connectFlow, ($flow) => {
  if (!$flow) return null;
  return $flow.step === "key"
    ? `API key ${$flow.provider} (masked)`
    : `model (Enter = ${$flow.defaultModel})`;
});

/** Agents en cours par chat (0-based) pour la sidebar `A{n}`. */
export function runningAgents(chat: number): number {
  return get(agents).filter((a) => a.owner === chat && a.status === "running")
    .length;
}

// ------------------------------------------------------------- cycle de vie

let started = false;
let stopListen: Unlisten[] = [];
let fxTimer: ReturnType<typeof setInterval> | null = null;
let scareTimer: ReturnType<typeof setTimeout> | null = null;

/** Charge l'état initial + écoute `crypte-event` / `crypte-ui`. */
export async function initChat(): Promise<void> {
  if (started) return;
  started = true;
  stopListen = [
    await listenCrypte(CRYPTE_EVENT, (payload) => void onDomain(payload)),
    await listenCrypte(CRYPTE_UI, (payload) => void onUi(payload)),
  ];
  // Tick peur calqué sur fx.rs (100 ms, +7 en effroi, −2 au calme).
  fxTimer = setInterval(() => {
    flash.update((f) => Math.max(0, f - 1));
    fear.update((v) =>
      get(spooked) ? Math.min(100, v + 7) : Math.max(0, v - 2),
    );
  }, 100);
  await Promise.all([
    refreshChats(),
    refreshAgents(),
    refreshApparition(),
    refreshConnect(),
    loadKiro(),
  ]);
}

export function destroyChat(): void {
  started = false;
  for (const un of stopListen) un();
  stopListen = [];
  if (fxTimer) clearInterval(fxTimer);
  if (scareTimer) clearTimeout(scareTimer);
  fxTimer = null;
  scareTimer = null;
}

// ------------------------------------------------------------------ events

/** Event domaine : on rafraîchit via les commandes (jamais de patch local). */
async function onDomain(ev: CrypteEvent): Promise<void> {
  switch (ev.type) {
    case "chat_created":
    case "chat_closed":
      await refreshChats();
      break;
    case "user_msg":
    case "assistant_msg":
    case "assistant_error":
    case "compacted":
    case "web_answer":
      if (ev.chat - 1 === get(activeIndex)) {
        await refreshState(get(activeIndex));
        await refreshGauge();
      }
      await refreshChats();
      break;
    case "agent_spawned":
    case "agent_ask":
    case "agent_done":
    case "agent_failed":
      await refreshAgents();
      break;
    case "mailbox":
      notice.set(`[${ev.canal}/${ev.priorite}] ${ev.texte}`);
      break;
    case "started":
      break;
  }
}

/** Event UI éphémère : apparition, critique, confirm outil, hub. */
function onUi(ev: CrypteUiEvent): void {
  switch (ev.kind) {
    case "apparition":
      apparition.set(ev.state);
      if (ev.state.omen === "scare") {
        // F5 shell (overlay) + fx locale (flash/brouillard) ensemble.
        triggerSpookFx();
      }
      break;
    case "critic":
      critic.set(ev.text);
      break;
    case "tool_confirm":
      pendingConfirm.set(ev.request);
      break;
    case "tool_delivery":
      pendingConfirm.set(null);
      break;
    case "hub_tick":
      hubSnapshot.set(ev.snapshot);
      break;
    // spec_progress, haunt_frame, vps_outcome, shop, mcp_refreshed :
    // owned par les tranches kiro / haunt / vps / shop.
    default:
      break;
  }
}

// ------------------------------------------------------------------- chats

export async function refreshChats(): Promise<void> {
  chats.set(await invokeCmd<ChatSummary[]>("chat_list"));
  const list = get(chats);
  if (list.length === 0) {
    chatState.set(null);
    return;
  }
  const clamped = Math.min(get(activeIndex), list.length - 1);
  activeIndex.set(clamped);
  await refreshState(clamped);
}

export async function refreshState(index: number): Promise<void> {
  chatState.set(await invokeCmd<ChatState>("chat_state", { index }));
}

export async function selectChat(index: number): Promise<void> {
  const state = await invokeCmd<ChatState>("chat_select", { index });
  activeIndex.set(index);
  chatState.set(state);
  notice.set(null);
  await refreshGauge();
}

export async function newChat(): Promise<void> {
  const created = await invokeCmd<ChatSummary>("chat_new");
  await refreshChats();
  await selectChat(created.index);
}

export async function closeChat(index: number): Promise<void> {
  chats.set(await invokeCmd<ChatSummary[]>("chat_close", { index }));
  const list = get(chats);
  if (list.length === 0) {
    chatState.set(null);
    return;
  }
  await selectChat(Math.min(index, list.length - 1));
}

export async function clearChat(): Promise<void> {
  const index = get(activeIndex);
  chatState.set(await invokeCmd<ChatState>("chat_clear", { index }));
}

/** Envoi : ACK immédiat, résultat par event (jamais de patch local). */
export async function send(text: string): Promise<void> {
  const body = text.trim();
  if (!body) return;
  const index = get(activeIndex);
  const ack = await invokeCmd<{ accepted: boolean; reason: string | null }>(
    "chat_send",
    { index, text: body },
  );
  if (!ack.accepted) {
    notice.set(ack.reason ?? "send refused");
    return;
  }
  notice.set(null);
  await refreshState(index);
}

export async function stop(): Promise<boolean> {
  return invokeCmd<boolean>("chat_stop", { index: get(activeIndex) });
}

export async function compact(): Promise<void> {
  const index = get(activeIndex);
  await invokeCmd<CompactResult>("chat_compact", { index });
  await refreshState(index);
}

export async function refreshGauge(): Promise<void> {
  const list = get(chats);
  if (list.length === 0) {
    gauge.set(null);
    return;
  }
  gauge.set(
    await invokeCmd<{ used: number; window: number }>("ctx_gauge", {
      index: get(activeIndex),
    }),
  );
}

export async function refreshAgents(): Promise<void> {
  agents.set(await invokeCmd<SubAgentInfo[]>("agents_list", { all: false }));
}

// ------------------------------------------------------- apparition + Kiro

export async function refreshApparition(): Promise<void> {
  apparition.set(await invokeCmd<ApparitionState>("apparition_state"));
}

/** Jump scare F5 : 2 s d'effroi (SCARE), éclair, Kiro effrayé. */
export async function spook(): Promise<void> {
  apparition.set(await invokeCmd<ApparitionState>("spook"));
  triggerSpookFx();
}

/** Fx locale d'un effroi (flash + brouillard 2 s). */
function triggerSpookFx(): void {
  spooked.set(true);
  flash.set(4);
  if (scareTimer) clearTimeout(scareTimer);
  scareTimer = setTimeout(() => spooked.set(false), 2000);
}

/** Clic sur Kiro (« hit him ») : `critic_smack`, bulle via event `critic`. */
export async function smackKiro(): Promise<void> {
  const res = await invokeCmd<{ text: string }>("critic_smack");
  critic.set(res.text);
}

async function loadKiro(): Promise<void> {
  const [idle, fear] = await Promise.all([
    invokeCmd<Creature>("bestiary_get", { slug: "kiro-spectre" }),
    invokeCmd<Creature>("bestiary_get", { slug: "kiro-effroi" }),
  ]);
  kiroIdle.set(idle);
  kiroSpooked.set(fear);
}

// ----------------------------------------------------------------- connect

export async function refreshConnect(): Promise<void> {
  const [status, list] = await Promise.all([
    invokeCmd<ConnectStatus>("connect_status"),
    invokeCmd<ProviderInfo[]>("providers_list"),
  ]);
  connectStatus.set(status);
  providers.set(list);
}

/** Démarre le flow `/connect <provider>` (clé → modèle, brouillon intact). */
export function startConnect(provider: string): void {
  const known = get(providers).find((p) => p.name === provider);
  connectFlow.set({
    provider,
    step: "key",
    key: "",
    buffer: "",
    defaultModel: known?.model ?? "?",
  });
}

export function connectType(buffer: string): void {
  connectFlow.update((f) => (f ? { ...f, buffer } : f));
}

/** Entrée dans le flow : clé → modèle, modèle → `connect_save`. */
export async function connectCommit(): Promise<void> {
  const flow = get(connectFlow);
  if (!flow) return;
  if (flow.step === "key") {
    connectFlow.set({ ...flow, step: "model", key: flow.buffer, buffer: "" });
    return;
  }
  const model = flow.buffer.trim() || undefined;
  const res = await invokeCmd<{
    ok: boolean;
    error: string | null;
    status: ConnectStatus;
  }>("connect_save", { provider: flow.provider, key: flow.key, model });
  connectStatus.set(res.status);
  connectFlow.set(null);
  notice.set(
    res.ok
      ? `connected: ${res.status.provider} / ${res.status.model ?? "default"}`
      : (res.error ?? "connection failed"),
  );
}

/** Échap : `connexion annulée`, prompt restauré. */
export function cancelConnect(): void {
  if (!get(connectFlow)) return;
  connectFlow.set(null);
  notice.set("connection cancelled");
}
