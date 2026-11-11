// contracts.ts — types TS de référence du portage Crypte Tauri v2 + Svelte.
// Source : src TUI /home/baptiste/tmp/crypte-mailbox (feat/mailbox-agents @b75a640)
// lue le 2026-11-08. Tout invoke/écoute DOIT utiliser ces types, sans duplication.
// Règles : aucun secret (clé API, secret jeton) ne transite ici, sauf
// JetonCreated.secret (affiché 1x, jamais stocké ni loggué).

// ---------------------------------------------------------------- palette
// Miroir exact de src/theme.rs (qté en hex, without # in Rust -> with # here).
export const CRYPTE_THEME = {
  void: "#0C0908",
  crypt: "#1E1512",
  bone: "#EBDDC3",
  sand: "#BAA685",
  umber: "#4C4437",
  ash: "#6E6862",
  pumpkin: "#F2701E",
  blood: "#D7263D",
  dried: "#6E101A",
  ecto: "#7CF29A",
  spectre: "#9B8CFF",
  candle: "#FFC46B",
  ink: "#120D0B",
  pumpkinDark: "#963E12",
  ectoDark: "#2D4E34",
  spectreDark: "#42396E",
  candleDark: "#AC783C",
  burlap: "#6E583A",
  moss: "#58D668",
  wisp: "#4AA8F0",
} as const;
export type CrypteColor = keyof typeof CRYPTE_THEME;

// ---------------------------------------------------------------- chat
export type Role = "user" | "assistant" | "system"; // chat/mod.rs
export interface ChatMessage {
  role: Role;
  text: string;
}
export interface PendingQuestion {
  // question.rs
  question: string;
  options: string[];
}
export interface ChatSummary {
  index: number; // 0-based interne ; affichage = index+1
  name: string;
  model: string | null; // null = défaut provider
  sending: boolean;
  questionPending: boolean;
  promptTokens: number;
  completionTokens: number;
}
export interface ChatState extends ChatSummary {
  messages: ChatMessage[];
  input: string;
  pendingQuestion: PendingQuestion | null;
}
export interface SendAck {
  accepted: boolean;
  reason: string | null; // ex "not connected…", "no chat"
}
export interface CompactResult {
  before: number;
  after: number;
}
export interface UsageReport {
  // /usage : chat + session + almanach
  chat: DayTally;
  session: DayTally;
  almanacTotal: DayTally;
  almanacDays: number;
}
export interface DayTally {
  // almanac.rs
  prompt: number;
  completion: number;
  calls: number;
}

// ------------------------------------------------------- agents + bus (TUI)
export type AgentStatus = "running" | "done" | "failed"; // sessions/subagents.rs
export interface SubAgentInfo {
  id: number; // 1-based global
  owner: number; // chat index 0-based
  deliver: number; // chat index 0-based
  status: AgentStatus;
  prompt: string;
  lastText: string | null;
}
export type LinkEnd = // bus.rs (chat = index 0-based, agent = id global)
  | { kind: "chat"; id: number }
  | { kind: "agent"; id: number };
export interface BusLink {
  from: LinkEnd;
  to: LinkEnd;
}
export interface BusState {
  links: BusLink[];
  forwarded: number;
  capped: number;
}
export type MsgTarget = string; // "2" | "@2" | "#3" | "all" (parse_target)

// ------------------------------------------------------- providers / connect
export interface ProviderInfo {
  // settings store : JAMAIS la clé
  name: string;
  baseUrl: string;
  model: string;
  hasKey: boolean;
  known: boolean; // true = un des 12 KNOWN (known.rs)
}
export interface ConnectStatus {
  connected: boolean;
  provider: string | null;
  model: string | null;
}
export interface ConnectResult {
  ok: boolean;
  error: string | null; // sans secret
  status: ConnectStatus;
}

// ---------------------------------------------------------------- web (/web)
export interface SearchResult {
  title: string;
  url: string;
  snippet: string;
}
export interface WebAnswer {
  backend: string; // Tavily | Brave | DuckDuckGo | Wikipedia
  text: string;
  sources: SearchResult[]; // <= 5
}

// ------------------------------------------------------- outils / permissions
export interface ToolDef {
  name: string;
  description: string;
}
export type ToolClass = "read" | "change"; // tools/mod.rs Class
export type PermMode = "auto" | "yolo"; // permissions.rs
export type PermRule = "allow" | "ask" | "deny";
export interface PermRuleEntry {
  pattern: string;
  rule: PermRule;
}
export interface PermissionsState {
  mode: PermMode;
  rules: PermRuleEntry[];
}
export interface ToolCallView {
  name: string;
  argsJson: string;
}
export interface ToolConfirmRequest {
  // 1 seul ToolRun à la fois (flow.rs) : pas d'id, un seul pendant
  chat: number;
  call: ToolCallView;
}
export interface ToolDelivery {
  call: ToolCallView;
  ok: boolean;
  text: string;
  ms: number;
}

// ---------------------------------------------------------------- kiro (.kiro/)
export interface KiroStatus {
  projectDir: string;
  globalDir: string | null;
  steering: number;
  specs: number;
  hooks: number;
  mcpServers: number;
  skills: number;
  text: string; // = status::text (rendu FR)
}
export interface Skill {
  name: string;
  description: string;
}
export interface Snapshot {
  // kiro/checkpoint.rs
  name: string;
  created: string; // ISO ou brut disque
}
export interface SpecTask {
  label: string;
  done: boolean;
}
export interface Spec {
  name: string;
  tasks: SpecTask[];
  done: number;
  total: number;
}
export interface McpServerState {
  name: string;
  enabled: boolean;
  tools: number;
}
export interface McpStatus {
  servers: McpServerState[];
}
export interface NamedHooks {
  file: string;
  triggers: string[];
  count: number;
}
export interface HooksState {
  hooks: NamedHooks[];
  recent: string[]; // derniers déclenchements (texte)
}

// ------------------------------------------------------- boutique / bestiaire
export type ShopEffect = "calm" | "scare" | "apparition" | "flash"; // shop.rs
export interface ShopItem {
  name: string;
  lore: string;
  price: number;
  effect: ShopEffect;
  calm?: number; // renseigné si effect == "calm"
}
export interface ShopState {
  items: ShopItem[];
  earned: number; // 1 token consommé = 1 pièce
  spent: number;
  balance: number; // jamais négatif
  message: string | null;
}
export interface ShopBuyResult {
  ok: boolean;
  message: string;
  state: ShopState;
}
export interface Creature {
  // bestiary.rs : 33 créatures
  slug: string;
  name: string;
  lore: string;
  grid: string[]; // 32 lignes de 32 lettres (fichier assets/pixel/<slug>.txt)
}
export interface ApparitionState {
  creature: Creature;
  omen: "error" | "compact" | "scare" | null;
}

// ---------------------------------------------------------------- hub / haunt
export type Mood = "idle" | "waiting" | "running" | "done" | "failed"; // hub.rs
export interface Spirit {
  key: string; // "chat:2" | "agent:3"
  x: number; // dalles (ROOM=10, iso côté frontend)
  y: number;
  mood: Mood;
  bubble: string | null;
}
export interface Orb {
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
  progress: number; // 0..1
}
export interface HubSnapshot {
  spirits: Spirit[];
  orbs: Orb[];
}
export type HauntKey =
  | "left"
  | "right"
  | "up"
  | "down"
  | "fire"
  | "quit"
  | string;
export interface HauntHud {
  level: number;
  score: number;
  lives: number;
  fear: number;
}
export interface HauntFrame {
  running: boolean;
  player: { x: number; y: number };
  foes: { x: number; y: number; kind: string }[];
  shots: { x: number; y: number; foe: boolean }[];
  hud: HauntHud;
}

// ---------------------------------------------------------------- VPS (/vps)
export interface VpsProfile {
  // vps.rs
  name: string;
  host: string;
  user: string;
  port: number;
  key: string | null;
  dir: string;
  dst: string; // user@host[:port]
}
export interface VpsState {
  profiles: VpsProfile[];
  current: string | null;
  statusByName: Record<string, string>; // dernier statut connu
}
export interface VpsAck {
  accepted: boolean;
  detail: string;
}

// ---------------------------------------------------------------- mailbox v2
// Formes stables = mbox/json.rs (ordres 10 clés, tail 5 clés, metriques…).
export type Priorite = "P0" | "P1" | "P2";
export type BusTopic = "phone" | "gpu" | "file" | "divers";
export interface MboxMessage {
  date: string;
  expediteur: string;
  priorite: Priorite;
  dest: string;
  corps: string;
}
export interface MboxTailAll {
  inbox: MboxMessage[];
  bus: Record<BusTopic, MboxMessage[]>;
}
export type MboxSendStatus = "sent" | "throttled" | "same-state";
export interface MboxSendResult {
  status: MboxSendStatus;
  detail: string;
}
export type StatutOrdre = "actif" | "revoque" | "expire"; // ordres.rs
export interface Ordre {
  id: string; // O-AAAAMMJJ-NNN
  to: string;
  ttl: number; // heures, 0 = permanent
  statut: StatutOrdre;
  src: string;
  texte: string;
  prio: Priorite | null;
  supersede: string | null;
  hash: string | null; // 40 hexa
  sig: string | null;
}
export type Verdict = "lu" | "applique" | "refuse"; // ack.rs
export interface HbEntry {
  nom: string;
  etat: string;
  epoque: number;
  frais: boolean; // < 900 s
}
export interface DoctorCheck {
  ok: boolean;
  label: string;
  detail: string;
}
export interface Metriques {
  compteurs: Record<string, number>;
  agents: Record<string, Record<string, number>>;
  integrite: {
    ordres: { ok: number; ko: number; sans: number };
    signatures: Record<string, number>;
  };
}
export interface PisteSpan {
  runId: string;
  epoque: number;
  agent: string;
  action: string;
  ordreId: string;
  verdict: string;
}
export interface JetonCreated {
  nom: string;
  secret: string; // montré 1x : afficher, ne jamais stocker/logguer
}
export interface JetonInfo {
  nom: string;
  revoque: boolean;
}
export interface VagueState {
  // vague.rs : file ID|APRES|COMMANDE
  pending: number;
  running: number;
  done: number;
  detail: string;
}

// ---------------------------------------------------------------- events
// Canal 1 "crypte-event" : réutilise events.rs (tag snake_case) + Mailbox.
// Canal 2 "crypte-ui" : éphémère UI (confirmations, frames, snapshots).
export type CrypteEvent =
  | { type: "started"; ts: number; version: string }
  | { type: "chat_created"; ts: number; chat: number; name: string }
  | { type: "chat_closed"; ts: number; chat: number; name: string }
  | { type: "user_msg"; ts: number; chat: number; text: string }
  | {
      type: "assistant_msg";
      ts: number;
      chat: number;
      text: string;
      promptTokens: number;
      completionTokens: number;
    }
  | { type: "assistant_error"; ts: number; chat: number; error: string }
  | {
      type: "agent_spawned";
      ts: number;
      agent: number;
      owner: number;
      deliver: number;
      prompt: string;
    }
  | {
      type: "agent_ask";
      ts: number;
      agent: number;
      owner: number;
      deliver: number;
      text: string;
    }
  | {
      type: "agent_done";
      ts: number;
      agent: number;
      owner: number;
      deliver: number;
      text: string;
      promptTokens: number;
      completionTokens: number;
    }
  | {
      type: "agent_failed";
      ts: number;
      agent: number;
      owner: number;
      deliver: number;
      error: string;
    }
  | { type: "compacted"; ts: number; chat: number; before: number; after: number }
  | {
      type: "web_answer";
      ts: number;
      chat: number;
      backend: string;
      text: string;
    }
  | { type: "mailbox"; ts: number; canal: string; priorite: string; texte: string };

export type CrypteUiEvent =
  | { kind: "tool_confirm"; request: ToolConfirmRequest }
  | { kind: "tool_delivery"; chat: number; delivery: ToolDelivery }
  | { kind: "spec_progress"; name: string; label: string; done: number; total: number }
  | { kind: "hub_tick"; snapshot: HubSnapshot }
  | { kind: "apparition"; state: ApparitionState }
  | { kind: "haunt_frame"; frame: HauntFrame }
  | { kind: "vps_outcome"; target: string; op: string; ok: boolean; text: string }
  | { kind: "shop"; state: ShopState }
  | { kind: "mcp_refreshed"; status: McpStatus }
  | { kind: "critic"; text: string };
