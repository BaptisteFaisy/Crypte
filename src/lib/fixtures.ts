// fixtures.ts — réponses de dev hors Tauri (jamais de secret).
// Formes = contracts.ts. Utilisé par tauri.ts quand isTauri() est faux.
// Commandes chat minimales incluses (écran / affichable sans backend).

import type {
  ApparitionState,
  BusTopic,
  ChatMessage,
  ChatState,
  ChatSummary,
  ConnectResult,
  ConnectStatus,
  Creature,
  HauntFrame,
  MboxMessage,
  MboxSendResult,
  Ordre,
  ProviderInfo,
  ShopBuyResult,
  ShopState,
  VpsProfile,
  VpsState,
} from "./contracts";

const MOCK_PROVIDERS: ProviderInfo[] = [
  { name: "anthropic", baseUrl: "https://api.anthropic.com", model: "sonnet", hasKey: true, known: true },
  { name: "openai", baseUrl: "https://api.openai.com/v1", model: "gpt-5", hasKey: false, known: true },
  { name: "local", baseUrl: "http://localhost:11434/v1", model: "qwen", hasKey: false, known: false },
];

let mockActiveProvider = "anthropic";
let mockSpent = 0;
const MOCK_EARNED = 1200;

const MOCK_SHOP: ShopState = {
  items: [
    { name: "Lightning flash", lore: "a flash with no scare", price: 200, effect: "flash" },
    { name: "Soothing candle", lore: "fear drops by 30", price: 300, effect: "calm", calm: 30 },
    { name: "Kiro's cry", lore: "Kiro panics: flash + apparition", price: 500, effect: "scare" },
    { name: "Apparition rite", lore: "a new creature emerges", price: 700, effect: "apparition" },
    { name: "Holy water", lore: "fear drops to zero", price: 900, effect: "calm", calm: 100 },
  ],
  earned: MOCK_EARNED,
  spent: 0,
  balance: MOCK_EARNED,
  message: null,
};

function mockShopState(message: string | null): ShopState {
  return {
    ...MOCK_SHOP,
    items: MOCK_SHOP.items.map((i) => ({ ...i })),
    spent: mockSpent,
    balance: MOCK_EARNED - mockSpent,
    message,
  };
}

/** Grille fantôme 32x32 générée (dev only, le backend renvoie les vraies). */
export function mockGrid(): string[] {
  return Array.from({ length: 32 }, (_, y) => {
    let row = "";
    for (let x = 0; x < 32; x += 1) {
      const inBody = x >= 8 && x < 24 && y >= 4 && y < 26;
      const inHead = (x - 15.5) ** 2 + (y - 10) ** 2 < 64;
      const eye = y >= 9 && y < 12 && ((x >= 12 && x < 14) || (x >= 17 && x < 19));
      if (eye) row += "K";
      else if (inBody || inHead) row += "W";
      else row += ".";
    }
    return row;
  });
}

function mockCreature(slug: string): Creature {
  return {
    slug,
    name: slug === "fantome" ? "Kiro" : slug,
    lore: "Dev fixture: the backend serves the true sprite.",
    grid: mockGrid(),
  };
}

const MOCK_VPS: VpsState = {
  profiles: [
    { name: "r630", host: "100.124.18.48", user: "baptiste", port: 22, key: null, dir: "/home/baptiste/crypte", dst: "baptiste@100.124.18.48" },
  ],
  current: null,
  statusByName: { r630: "unknown" },
};

const MOCK_ORDRES: Ordre[] = [
  {
    id: "O-20261108-001", to: "ALL", ttl: 24, statut: "actif",
    src: "test", texte: "Veiller sur la crypte.", prio: "P1",
    supersede: null, hash: null, sig: null,
  },
];

const MOCK_INBOX: MboxMessage[] = [
  { date: "2026-11-08T17:40:00", expediteur: "freebuff", priorite: "P1", dest: "ALL", corps: "Failover duello-secours OK." },
];

function mockBus(topic: BusTopic): MboxMessage[] {
  if (topic === "file") {
    return [{ date: "2026-11-08T17:41:00", expediteur: "freebuff", priorite: "P2", dest: "file", corps: "capture-031.png (64 Kio, meta seule)." }];
  }
  return [];
}

function mockHauntFrame(running: boolean): HauntFrame {
  return {
    running,
    player: { x: 16, y: 28 },
    foes: [{ x: 8, y: 6, kind: "bat" }],
    shots: [],
    hud: { level: 1, score: 0, lives: 3, fear: 10 },
  };
}

function mockApparition(): ApparitionState {
  return { creature: mockCreature("fantome"), omen: null };
}

function mockSendResult(): MboxSendResult {
  return { status: "sent", detail: "ok (fixture)" };
}

function mockStatus(): ConnectStatus {
  return { connected: true, provider: mockActiveProvider, model: "sonnet" };
}

function mockConnectResult(ok: boolean, error: string | null): ConnectResult {
  return { ok, error, status: mockStatus() };
}

function mockConnect(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "connect_status":
      return mockStatus();
    case "connect_save":
    case "connect_muse":
      return mockConnectResult(true, null);
    case "providers_list":
      return MOCK_PROVIDERS.map((p) => ({ ...p }));
    case "provider_select":
      mockActiveProvider = String(args["name"] ?? mockActiveProvider);
      return mockStatus();
    case "provider_add":
      MOCK_PROVIDERS.push({
        name: String(args["name"] ?? "custom"),
        baseUrl: String(args["baseUrl"] ?? ""),
        model: String(args["model"] ?? ""),
        hasKey: false,
        known: false,
      });
      return null;
    default:
      return mockVps(cmd, args);
  }
}

function mockVps(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "vps_list":
      return structuredClone(MOCK_VPS);
    case "vps_launch":
    case "vps_stop":
    case "vps_status":
    case "vps_install":
      return { accepted: true, detail: "fixture: voir event vps_outcome" };
    case "vps_add":
      MOCK_VPS.profiles.push(args["profile"] as VpsProfile);
      return null;
    case "vps_remove":
      MOCK_VPS.profiles = MOCK_VPS.profiles.filter(
        (p) => p.name !== String(args["name"]),
      );
      return null;
    default:
      return mockShop(cmd, args);
  }
}

function mockShopBuy(args: Record<string, unknown>): ShopBuyResult {
  const item = MOCK_SHOP.items.find((i) => i.name === String(args["item"]));
  if (!item) throw `unknown item ${String(args["item"])}`;
  if (MOCK_EARNED - mockSpent < item.price) {
    const state = mockShopState("Not enough coins.");
    return { ok: false, message: "Not enough coins.", state };
  }
  mockSpent += item.price;
  const state = mockShopState(`${item.name} acquired.`);
  return { ok: true, message: `${item.name} acquired.`, state };
}

function mockShop(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "shop_state":
      return mockShopState(null);
    case "shop_buy":
      return mockShopBuy(args);
    case "bestiary_list":
      return [
        { slug: "fantome", name: "Kiro", lore: "The keeper of the crypt." },
        { slug: "citrouille", name: "Pumpkin", lore: "Its cross eyes never forgive." },
        { slug: "chauve-souris", name: "Bat", lore: "It sleeps upside down." },
      ];
    case "bestiary_get":
      return mockCreature(String(args["slug"] ?? "fantome"));
    case "bestiary_random":
      return mockCreature("citrouille");
    case "apparition_state":
      return mockApparition();
    case "haunt_start":
      return mockHauntFrame(true);
    case "haunt_stop":
    case "haunt_input":
      return null;
    default:
      return mockMbox(cmd, args);
  }
}

function mockMboxOrdre(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "mbox_ordre_new":
    case "mbox_ordre_delegate":
      return { id: "O-20261108-999" };
    case "mbox_ordre_list":
    case "mbox_ordre_pull":
      return MOCK_ORDRES;
    case "mbox_ordre_revoke":
    case "mbox_ordre_ack":
      return null;
    default:
      return mockMboxOps(cmd, args);
  }
}

function mockMboxOps(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "mbox_doctor":
      return [{ ok: true, label: "inbox", detail: "fixture OK" }];
    case "mbox_jeton_new":
      return { nom: String(args["name"] ?? "agent"), secret: "FIXTURE-NEVER-STORE" };
    case "mbox_jeton_revoke":
      return null;
    case "mbox_jeton_list":
      return [{ nom: "freebuff", revoque: false }];
    case "mbox_jeton_purge":
      return { purged: 0 };
    case "mbox_metriques":
      return { compteurs: { send_ok: 1 }, agents: {}, integrite: { ordres: { ok: 1, ko: 0, sans: 0 }, signatures: {} } };
    case "mbox_piste":
      return [];
    case "mbox_vague":
      return { pending: 0, running: 0, done: 0, detail: "fixture" };
    case "mbox_fantome":
      return { ok: true, detail: "fixture" };
    default:
      return mockChat(cmd, args);
  }
}

const MOCK_CHAT_SUMMARY: ChatSummary = {
  index: 0,
  name: "dev",
  model: null,
  sending: false,
  questionPending: false,
  promptTokens: 0,
  completionTokens: 0,
};

function mockChatState(): ChatState {
  const hello: ChatMessage = { role: "assistant", text: "Dev fixture: le backend sert les vrais chats." };
  return { ...MOCK_CHAT_SUMMARY, messages: [hello], input: "", pendingQuestion: null };
}

function mockChat(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "chat_list":
      return [{ ...MOCK_CHAT_SUMMARY }];
    case "chat_new":
      return { ...MOCK_CHAT_SUMMARY };
    case "chat_select":
    case "chat_clear":
    case "chat_state":
      return mockChatState();
    case "chat_close":
      return [];
    case "chat_send":
      return { accepted: true, reason: null };
    case "chat_stop":
      return false;
    case "chat_compact":
      return { before: 0, after: 0 };
    case "chat_set_model":
    case "chat_set_thinking":
      return String(args["model"] ?? args["level"] ?? "dev");
    case "usage_get":
      return {
        chat: { prompt: 0, completion: 0, calls: 0 },
        session: { prompt: 0, completion: 0, calls: 0 },
        almanacTotal: { prompt: 0, completion: 0, calls: 0 },
        almanacDays: 0,
      };
    case "ctx_gauge":
      return { used: 0, window: 200000 };
    case "agents_list":
      return [];
    case "agent_ask":
    case "tell_send":
    case "msg_send":
    case "link_add":
      return null;
    case "link_remove":
      return false;
    case "links_list":
      return { links: [], sent: {}, received: {} };
    case "spook":
      return mockApparition();
    case "critic_smack":
      return "Miaou. (fixture)";
    case "web_search":
      return { text: "fixture: aucun réseau en dev.", backend: "ddg", promptTokens: 0, completionTokens: 0 };
    default:
      throw `no dev fixture for ${cmd}`;
  }
}

function mockMbox(cmd: string, args: Record<string, unknown>): unknown {
  switch (cmd) {
    case "mbox_tail":
      if (args["all"] === true) {
        return {
          inbox: MOCK_INBOX,
          bus: { phone: [], gpu: [], file: mockBus("file"), divers: [] },
        };
      }
      if (typeof args["bus"] === "string") return mockBus(args["bus"] as BusTopic);
      return MOCK_INBOX;
    case "mbox_send":
    case "mbox_bus":
      return mockSendResult();
    case "mbox_digest":
      return { digests: 0 };
    case "mbox_rotate":
    case "mbox_hb":
      return { ok: true };
    case "mbox_hb_list":
      return [{ nom: "freebuff", etat: "veille", epoque: Date.now() / 1000, frais: true }];
    default:
      return mockMboxOrdre(cmd, args);
  }
}

/** Fixture par nom de commande (dev hors Tauri uniquement). */
export function mockFor(cmd: string, args: Record<string, unknown>): unknown {
  return mockConnect(cmd, args);
}
