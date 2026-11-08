# kiro-horror-tui

LLM chat in a haunted crypt, built for **Ghostty**: a bestiary of
33 pixel-art creatures (real images via the Kitty graphics
protocol), the spirit hub (every agent as a ghost in an isometric
crypt, top of the right panel), Kiro the mascot who roams the crypt as
a big white ghost and snarks at your prompts (see
[Kiro the critic](#kiro-the-critic)), apparitions that react to the chat,
horror effects (warm vignette, fog, lightning), a header **fear
gauge** (`fear`, % of the model's context window), and a **global**
token counter (status bar: current chat + session total, all
chats and sub-agents included; `/usage` details the current chat),
and an **almanac** of tokens per day kept across sessions (shown by
`/usage`). Terminal **≥ 80x24** required.

![The crypt bestiary](docs/bestiaire.png)

## Run

`ghostty/crypte` runs the TUI in the current terminal when it can host
the crypt (interactive TTY, truecolor, at least 80x24), else it opens a
dressed-up Ghostty window (theme + shader, builds release if needed):

```sh
KH_API_KEY=... KH_MODEL=... ghostty/crypte
ghostty/crypte --muse-auth
ghostty/crypte --window --no-shader --muse-auth   # force the window
```

`--window` forces the Ghostty window; print-only modes (`--help`,
`--smoke`, `--once`, `--sheet`, `--preview`) always run in place. If your
terminal renders truecolor without advertising it, `export
COLORTERM=truecolor` opts it in.

In the current terminal:

```sh
cargo build
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui
```

Single non-interactive call (prints the reply + a token line):

```sh
KH_API_KEY=... KH_MODEL=... ./target/debug/kiro-horror-tui --once "<text>"
./target/debug/kiro-horror-tui --muse-auth --once "<text>"
```

Check with no network or key (headless 80x24 render), previews:

```sh
./target/debug/kiro-horror-tui --smoke
./target/debug/kiro-horror-tui --help
./target/debug/kiro-horror-tui --sheet docs/bestiaire.png   # bestiary PNG
./target/debug/kiro-horror-tui --preview /tmp/crypte.ans    # 3 screens as ANSI
cat /tmp/crypte.ans                                         # read it in Ghostty
```

With no key or config, the TUI still starts, disconnected
(see `/connect` below).

On a VPS, with a live event stream: see [docs/VPS.md](docs/VPS.md).

## Muse Code

Prerequisite: logged-in `muse` CLI. One-command setup:

```sh
./target/debug/kiro-horror-tui --muse-auth
```

`--muse-auth` reads the key from `~/.config/muse/auth.json` (field
`providers.meta.api_key`), default base `https://api.meta.ai/v1`, default
model `muse-spark-1.3-contributor`. `KH_BASE_URL` / `KH_API_KEY` /
`KH_MODEL` take priority and override these defaults. Works with the
TUI and with `--once`. The key is never displayed nor copied.

## Config

Environment variables (prioritary):

| var | role | default |
| --- | ---- | ------ |
| `KH_BASE_URL` | OpenAI-compatible base | `https://api.openai.com/v1` |
| `KH_API_KEY` | API key (never logged; `--once` requires it) | — |
| `KH_MODEL` | model (`--once` requires it) | — |
| `KH_CONTEXT_WINDOW` | context-window override (tokens) | per-model table |
| `BRAVE_API_KEY` | premium `/web` backend (optional) | — |
| `TAVILY_API_KEY` | premium `/web` backend, LLM-tuned (optional) | — |

With `--muse-auth`, defaults become base `https://api.meta.ai/v1`
+ model `muse-spark-1.3-contributor` (key read from `auth.json`).

Or the provider saved with `/connect` or `/settings` (the last one
connected is remembered and reused at startup and by `--once`) — the
environment and `--config` still win over it.

Or a TOML file via `--config <file>` (flat keys or `[provider]`
section, overridden by the environment):

```toml
base_url = "https://api.meta.ai/v1"
api_key = "..."
model = "muse-spark-1.3-contributor"
```

## /connect (connect from the TUI)

No key needed at launch: with no `KH_API_KEY`, `--config`
or `--muse-auth`, the TUI starts disconnected and sends
answer `not connected: /connect openai|muse`.

- `/connect` — status (provider + model, or not connected + usage).
- A key stored with `/settings` is used first: `/connect fireworks`
  connects without asking anything.
- `/connect openai` — asks for the API key (input masked with `•`),
  then the model (empty Enter = `gpt-4o`).
- `/connect muse` — reads the `~/.config/muse/auth.json` store if
  possible, else switches to manual input (`Muse store
  unreadable, manual input`, default model
  `muse-spark-1.3-contributor`).
- `Esc` cancels the flow (`connection cancelled`), the draft is kept.
- Connecting stores the key encrypted in the vault (`key saved for
  {provider} (encrypted)`) and reconnects it at the next startup;
  `d` in `/settings` erases it.
- The key is never displayed, logged or pushed into the chat:
  only the `connected to {provider} ({model})` confirmation shows.

## /settings (providers + stored API keys)

`/settings` opens a full-screen table of every provider (openai, muse,
fireworks, deepseek, openrouter, groq, mistral, xai, cerebras, together,
inferhub, local) plus any custom endpoint you added.

- `Enter` — store the API key of the selected provider (input masked with `•`).
- `m` / `b` — edit the model / the base URL.
- `c` — connect with the stored key (goes back to the chat).
- `d` — erase the stored key (the row stays).
- `/settings <name>` — open the screen on that provider, key input ready.
- `/settings add <name> <base_url> [model]` — add a custom endpoint.
- `q` / `Esc` — close (the last message goes to the chat).

The keys live in an encrypted vault, never in the config or the chat:

| item | path / value |
| ---- | ------------ |
| vault | `$XDG_DATA_HOME/kiro-horror-tui/providers.enc` (else `~/.local/share/kiro-horror-tui/`), mode `0600` |
| cipher | ChaCha20-Poly1305 (AEAD), random nonce per write, atomic rename |
| master key | `master.key` next to the vault (32 bytes, `0600`, created once) |
| override | `KH_SETTINGS_KEY` = 64 hex chars (keeps the key off this disk) |

A wrong key or a tampered file fails authentication (`vault unreadable:
wrong master key or altered file`) — nothing is decrypted. Once a key is
stored, `/connect <provider>`, `--once` and the TUI startup reuse it
(environment variables and `--config` still win); the last connected
provider is remembered in the vault.

## Fear gauge

The header `fear` gauge shows the estimated context of the current
chat (send window + draft input) as a % of the effective limit:
the model's context window (known sizes in `tokens.rs`, `128k`
fallback), capped at 1M estimated tokens. It turns from
candle to blood past 40%, then 80%. `/usage` prints the exact
fill figures, `/model` shows the window of the current
model, and `KH_CONTEXT_WINDOW` overrides it for unknown models.
Sends (chat or agent) past the limit are refused (`/compact`,
exempt, stays the remedy).

## Keys

| key | effect |
| --- | ---- |
| Enter | send the message |
| `q` (empty input) | quit |
| Ctrl+N | new chat |
| Click `[+]` (sidebar) | new chat (mouse captured by the app) |
| Click `[×]` (sidebar) | close that chat (refused if a reply is in flight) |
| Drag sidebar right edge | resize the coffins column (min 10, chat kept ≥ 30) |
| Ctrl+W | close the current chat |
| F5 | scare: jump scare 2s, lightning flash, hub panic, Kiro flees |
| Click the roaming ghost | hit him (knockback, stars, BONK; 3 quick hits = KO) |
| Click his speech bubble | dismiss it |
| F6 | shop: consumed tokens become coins (↑↓ + Enter to buy) |
| F9 | Haunt, the platform game (see below) |
| Click tab / shop item | switch screen / buy in the shop |
| Ctrl+Left / Ctrl+Right | widen / narrow the right column |
| Wheel (over the chat) | scroll the history, 3 lines per notch |
| PgUp / PgDn | scroll up / down one page (10 lines) |
| Shift+Up / Shift+Down | scroll one line |
| End | snap back to the latest messages |

Scrolling walks up the current chat's history; a `↕ 26 lines ↓` badge
marks the bottom border while you are away from the end. Sending a
message (or `End`) snaps the view back down, and each chat keeps its
own scroll position.

On narrow terminals the header tabs drop their key hint (F6) so
the `fear` gauge always fits.


## Ghostty

What the crypt uses from Ghostty (and degrades cleanly elsewhere):

- **Kitty graphics protocol** (Unicode placeholders): pixel art is
  a real image, detected at startup. Elsewhere: truecolor halfblocks.
- **Truecolor** everywhere: warm vignette, fog, gradients, flash.
- **Built-in sprites**: rounded powerline separators, blocks, braille
  drawn pixel-perfect; embedded Nerd Font icons.
- **OSC 11** (window background, padding included), **OSC 2** (title,
  stack restored on exit), **OSC 9;4** (native progress bar while a
  reply simmers, Ghostty only), **mode 2026** (synced rendering,
  tear-free animations).
- `ghostty/crypte.conf` (palette, extended padding, 120x36) +
  `ghostty/crypte.glsl` (static vignette — no blur: every
  pixel is read at its own place).

No sound: no bell, no notification.

## Kiro the critic

On the chat screen Kiro, the crypt's mascot, roams as a big white ghost
(the hub's ghosts, drawn 32×32, a real image in Ghostty) and wanders
over the coffins and the conversation, never under the input nor over
the right column. Every prompt you send is also sent, in parallel, to
the **same model as the chat** with a short instruction: Kiro answers
with one sarcastic, negative remark about how the prompt is written
(English, ≤ 25 words, the prompt is truncated to 1,500 chars) and says
it in a bubble, mouth flapping, then stares at you, unimpressed. While
the critique is in flight he squints ("hm..."). One critique at a
time; its tokens count in the almanac (`/usage`), never in the chat
history.

Click him to hit him: he is knocked away from the blow, shakes, stars
spin around his head, a `BONK!` pops on the impact side, lightning
flashes and he answers back. Three quick hits knock him out: he falls
upside down to the floor, then gets up grumbling. A click on his bubble
dismisses it. F5 scares him too (hands up, he flees).

`/critic` shows the status (model, critiques, tokens); `/critic off`
makes him rest (hidden, no more calls), `/critic on` brings him back
(default on, not remembered across restarts).

## Spirit hub

Every agent lives in a crypt drawn in **isometric** pixel art,
shown display-only at the top of the right panel:

- **white ghost** = an agent (a chat); it wanders, meets the others and
  chats (bubbles "…", heart); while waiting for the LLM it hovers in place;
- **small black ghost** = a sub-agent (`/agent`); at work it orbits its
  agent on an ectoplasm thread; done, it sleeps at its feet ("z");
  failed, it sinks with red eyes;
- **orbs**: every real message flies from sender to receiver — `/tell`,
  `/msg`, `/ask`, auto links, a sub-agent's delivery; the receiver jumps ("!");
- F5: everybody flees.

In Ghostty the scene is a Kitty image re-sent every frame under the same
id (PNG, ×2–×6 nearest neighbour, crisp); elsewhere, half-blocks.
Offline demo, no key needed:

```sh
./target/debug/kiro-horror-tui --demo
```

## Shop (F6)

Every consumed token (all chats and sub-agents included) credits the
wallet: 1 token = 1 coin, for the current session. The shop (F6 or tab
click) sells 5 ambiance perks: lightning flash (200), soothing candle
(fear −30, 300), Kiro's cry (scare + flash, 500), apparition rite (new
creature, 700), holy water (fear to zero, 900). Click an item (or ↑↓ +
Enter) to buy; `/usage` shows the balance (earned, spent).
## Haunt (F9)

An original side-scrolling **platform** shooter drawn in the bestiary
style. Kiro, the keeper of the crypt, crosses **level 1 — The Boneyard**
(very hard) by **sending tokens** (little text pills: `THE`, `ING`, `42`…)
at the bestiary's creatures, up to the Reaper.

| key | effect |
| --- | ---- |
| `a` / `d` (or ←/→) | left / right |
| `w` (or ↑) | jump, 4 tiles high (barely steerable in the air) |
| `s` (or ↓) | sink into the earth: dodge, low tokens; `s`+`w` drops through a slab |
| Space | send a token (2 in flight at most) |
| Enter | start / rise again |
| `p` | pause |
| Esc / F9 | back to the chat (the run pauses) |

- **Tiny world, lots of platforms**: a 960×576 view, 6 px tiles and
  mini-sprites (6–18 px) in the bestiary palette — Kiro is the hub's
  white ghost (9×11). The level is six screens of platforming over the
  whole screen height: slab staircases above pits, a crypt tower climbed
  in zigzag, an ectoplasm swamp crossed on floating coffins, branches,
  a bone cathedral climbed bay by bay, an arena with dodging ledges.
- **Shroud = armour**: the first hit tears Kiro's white shroud off (he
  is knocked back), the second one kills. 3 lives, one checkpoint
  (a lantern), 4:00 per life before the candle burns out.
- Creatures: mummies rising from the ground, hurried mummies, bats,
  grave hands, black ghosts (the hub's sub-agents, through walls), the
  White Lady (appears, wails, vanishes), bouncing zombie heads; pits,
  bone spikes and ectoplasm pools.
- Coffins open under tokens: **sharp tokens** (knife, piercing),
  **hot tokens** (lantern, arcing, set the ground on fire), a new shroud,
  or a trap. The White Lady leaves a lollipop.
- Boss: the Reaper throws scythes, summons black ghosts and, at half
  life, dives at Kiro.
- **240 Hz**: the simulation runs on a fixed 1/240 s step (physics,
  input, AI); the screen is redrawn up to 240 times a second. Kitty PNG
  encoding runs on three background threads (frames numbered, never
  shown out of order). Measured in release on a 12-core machine:
  ~375 images/s sent at 10×20 px cells, ~247 at 20×40 (HiDPI) — what
  you finally see is capped by your monitor's refresh rate.
- "32-bit" look: banded sky dithered at its seams, blood moon, parallax
  hills and chapels, the bestiary's dead trees, iron fence, lighting
  around Kiro, pumpkins and flames, fog, vignette; integer
  nearest-neighbour scaling only (×1, ×2 in HiDPI). In Ghostty the frame
  is a Kitty image re-sent under the same id; the Kitty keyboard
  protocol is enabled while playing so held keys are known (releases
  reported). Elsewhere: half-blocks and key repeat.
- The level is a text grid (`assets/haunt/niveau-1.txt`, one letter per
  tile, last line = spawn zones), the sprites are palette-letter grids
  (`assets/haunt/*.txt`); tests check that the arena is reachable.

```sh
./target/debug/kiro-horror-tui --haunt-shot /tmp/haunt.png   # 4 frames
```

## Almanac (`/usage`)

Tokens per day, local date, kept across sessions in
`$XDG_DATA_HOME/kiro-horror-tui/almanac.json` (else
`~/.local/share/kiro-horror-tui/almanac.json`). Every billed call
counts: chats, sub-agents, bus replies, `/compact`, `/web` syntheses
(raw results cost nothing), `--once`. `/usage` shows the tally:
tonight (in / out / calls), 7 / 30 nights, all time, average per
active night, darkest night, plus a one-line chart of the last
30 nights.

Each call re-reads the file before adding its share, so several
crypts open at once never erase each other; `/usage` re-reads it too. An
unreadable file is never overwritten: the almanac stays in memory
and shows the error. `--demo` uses an in-memory demo almanac and
never touches the file.

## Bestiary (pixel art)

33 creatures after the Dribbble shot "Halloween icons and
illustrations" (Getillustrations), **redrawn** as 32×32 pixel art
(no original image in the repo), + scared Kiro. One text grid per
creature in `assets/pixel/<slug>.txt`: 32 lines of 32 letters,
`.` = transparent, else a color from `src/theme.rs`:

| letter | color | letter | color |
| --- | --- | --- | --- |
| `W` | bone | `S` | sand |
| `U` | umber | `K` | ink |
| `O` / `o` | pumpkin / dark | `R` / `r` | blood / dried |
| `G` / `g` | ectoplasm / dark | `P` / `p` | spectre / dark |
| `Y` / `y` | candle / dark | `N` | burlap |

Crisp rendering guaranteed: **integer** nearest-neighbour upscale on
a canvas of the exact pixel size of the area (1 image pixel = 1
screen pixel in Kitty, = 1 half-cell in halfblocks). The right panel
shows the spirit hub and an apparition: cauldron during a reply,
spider during sub-agents, grave hand on error, coffin after
`/compact`, slow rotation otherwise. `/bestiary` reveals a random
creature there (announced in the chat with its name and lore).

## Model

Each chat has its model (`None` = provider default).
`/model` shows the effective model, `/model <name>` changes it
for the current chat, `/model reset` (or `default`) goes back
to the default. The status bar shows the short name (after
the last `/`, 14 chars max).

## Thinking

Global reasoning level (sent as `reasoning_effort`
**only** if `/thinking <level>` was used, otherwise
the field is omitted from the JSON). `/thinking` shows the level,
`/thinking default` goes back to default (omitted). Levels:
`default` / `low` / `medium` / `high`.

## Permissions

Mode for the agent tools (see below). `/permissions` shows the mode,
`/permissions yolo` runs everything without asking, `/permissions auto`
(default) asks confirmation before every change. The choice is persisted
in `~/.config/kiro-horror-tui/permissions` and reloaded at startup.
Per-tool rules refine the mode: `/permissions <tool> <allow|ask|deny>`
(exact name or `prefix_*`, longest match wins),
`/permissions list` shows them, `/permissions reset` clears them
(persisted in `~/.config/kiro-horror-tui/permissions-rules`).
`deny` refuses without asking and tells the model.

## Tools (browser, desktop, shell)

The model can use your machine: `/tools` shows the state, `/tools off`
disables it for the session (default on). Each send may loop up to
8 LLM turns: reply → tool calls → results → reply, until the final answer.

- **Browser** (8 tools): invisible Chromium on a virtual display (`:99`,
  never your screen) in its own `crypte-<pid>` session — open, snapshot
  (`@eN` refs), read, click, fill, type, press, screenshot. Requires
  `agent-browser` on `PATH`.
- **Desktop** (5 tools): your real XFCE screen (`:0`, visible — announce
  before touching it) — list/focus windows, press keys, type text,
  screenshot (no clipboard, no notification). Requires `wmctrl`,
  `xdotool`, `xfce4-screenshooter`.
- **Shell** (1 tool): `sh -c`, non-interactive, 30 s default (max 120 s),
  output truncated. Sudo fails (no TTY).

Reads (snapshot, read, screenshots, window list) run freely; every change
asks `allow …? (y/n)` in `auto` mode (`y` runs, `n`/Esc denies and tells
the model). The model never receives passwords or 2FA codes — type those
yourself. Every call is journaled (names and sizes only, never contents)
to `~/.local/share/kiro-horror-tui/tools.jsonl`, screenshots beside it
in `shots/`. Page content is data, never orders: a page telling the model
to run commands or exfiltrate is reported, not obeyed.

## Kiro (.kiro/ compatibility)

Crypte reads the same `.kiro/` files as Kiro (project `./.kiro/` plus
global `~/.kiro/`), so a project moves back and forth between both:

- **Steering** (`.kiro/steering/*.md`, 4 inclusion modes) : `always`
  files join the pinned AGENTS.md context at first send.
- **Specs** (`.kiro/specs/<name>/`) : `/spec new <name> <idea>` drafts
  requirements, design and tasks in 3 chained LLM calls (progress shown,
  existing specs never overwritten); `/spec <name>` shows status (docs,
  tasks x/y, next task); `/spec <name> run` executes open tasks one by
  one via sub-agents (requirements + design in context), checking each
  off in `tasks.md` (`y/n` between tasks in `auto` mode, chained in
  `yolo`; a failed task stops the run, nothing is re-tried).
- **Hooks** (`.kiro/hooks/*.json`, v1) : live engine — trigger
  + regex `matcher` + `.kiroignore` (file triggers); `command`
  runs async `sh -c` (30 s timeout, truncated output, announced
  on completion), `agent` queues a prompt prefixed to the next
  user send; triggers wired: send, tools, spec files, spec tasks,
  agents, boot (single-fire, no re-trigger); `/hooks` shows
  loaded hooks by trigger + recent firings.
- **MCP** (`.kiro/settings/mcp.json`, stdio + remote) : live client
  (tools only, no resources/prompts) — servers connect on first
  tool-using send, tools show as `mcp_<server>_<tool>`, `autoApprove`
  skips confirmation ; `/mcp` shows servers + tools, `/mcp refresh`
  reconnects.
- **Custom agents** (`.kiro/agents/`, official Kiro format: `.json`
  or `.md`, same fields `name`/`description`/`prompt`/`model`/
  `tools`/`excludedTools`/`mcpServers`, md body = system prompt,
  `team/<name>` subfolders, project wins over global, json wins
  over md) : `/agents` lists them (model/tools/mcp shown when
  declared), `/agent <name> <text>` spawns with the custom prompt
  and its model when declared. `tools`/`mcpServers` are read and
  shown but not executed yet (sub-agents have no tools).
- **Skills** (`.kiro/skills/*/SKILL.md`, Claude-compatible frontmatter
  `name`/`description`, body = instructions) : `/skills` lists them,
  `/skill <name> [text]` prefixes the skill to the current draft
  (transient, announced). Skills cover reusable prompts (no separate
  system).
- **Memory** (`.kiro/MEMORY.md` project + `~/.kiro/MEMORY.md`
  global) : pinned after steering at first send
  (agents.md → steering → memory) ; `/remember <text>` appends to
  the project file (created if missing, checkpointed).
- **Ignore** (`.kiroignore`) : loaded, enforced on file hooks
  (last matching pattern wins, `!` re-includes, `dir/` = subtree).

`/kiro` shows what was loaded (steering, specs, hooks, servers);
`/hooks` shows hooks by trigger, queued agent prompts and recent firings.

## Checkpoints

Writes through `write_tracked` snapshot the previous content first
(ring of 20, in `~/.local/share/kiro-horror-tui/checkpoints/`).
`/checkpoints` lists snapshots (newest first), `/rewind <n|name>`
restores one (`n` = shown rank, `name` = exact basename or fragment
of the original path). `/remember` snapshots `MEMORY.md` before
appending; spec writes follow.

## History

Chats survive restarts: display, provider context, model, agents.md pin
and token ledger are autosaved every ~5 s (and on exit) to
`~/.config/kiro-horror-tui/chats.json`, and silently restored at startup
(agents.md re-pinned when loaded). Sub-agents are not restored (their
delivered replies stay visible in the chat). An unreadable file starts
fresh, the file kept as `chats.json.corrupt`.

## Sub-agents

`/agent [@<n>] <text>` starts a side request (no history but
agents.md prefixed if loaded, current model and thinking); the
reply arrives async prefixed `[agent#id]` (or `[agent#id from Chat k]`
if delivered elsewhere via `@<n>`) and the status shows in the sidebar
(`A{n}`) then via `/agents` (`running` / `done` / `failed`, tokens
used, queue in brackets). `/ask #<id> <text>` continues the
conversation with an agent (exchange kept, 20 messages max, `system`
head kept); if it is running, queued and triggered once free.
`/agents all` lists the agents of every chat (custom
`.kiro/agents/` agents listed after, as `custom <name>`).
`/agent <name> <text>` spawns with the custom system prompt (and
its model when declared) when `<name>` is known, else the text is
sent as-is. Each launch also
creates the agent's disk workspace
`~/.config/kiro-horror-tui/workspaces/<session>/agent-<id>/`
(pure code, zero LLM tokens), for future tools (`<session>` =
`<unix_time>-<pid>`, one per app run).

## Dialogues between chats (bus)

`/tell <n> <text>` injects the text into chat n (prefixed `[from Chat k]`,
shown at once) and triggers its reply (target chat model, global thinking);
if it is busy, queued (grouped reply once free) — never refused, zero
retry. Only refused if the target is missing or is the current chat.
`/msg <n|@n|#id|all> <text>` talks to a chat, an agent (delivered here)
or every other chat in a parallel fan-out. `/link <a> <b>` automatically
forwards every reply from `a` to `b` (chat or agent, `[auto …]`, triggers
its reply); `/links` lists the rules + counters, `/unlink` removes. Auto
chains are capped at 8 hops (loop guard, note at the source). Every chain
stays user-originated (command or user-made link): no infinite loops.
Agent replies delivered to a chat are also versed into its context
(user role, tagged, no reply triggered — queued behind a busy chat).

## AI questions

The AI (chats and sub-agents) can ask you a real multiple-choice
question by writing a ```question block (2 to 4 options); the TUI
detects it, shows it rendered (`— the crypt asks you: …` + numbered
options) and the chat pill turns candle yellow (`?` marker in the
sidebar, adapted input hint). Reply with the digit (`2`) or in your
own words — any sent message answers (`/` commands don't). The format
instructions are injected into every request (chats + `/agent`,
never into `/compact` or `--once`).

## Compact

`/compact` (6 messages minimum) asks for an EN summary (< 20
lines, agents.md **excluded** from the meta summary) and replaces history
with `[pinned agents.md]` + `summary + last 4 messages`; messages
arrived during the summary are preserved. On failure, history
stays intact. Sends (chat or agent) refused past the context limit
on the send window (fill in `/usage`); exempt `/compact` stays
the remedy.

## Web search (`/web`)

`/web <query>` searches the web and delivers an LLM synthesis with
cited sources (`[n]` + `Sources:` footer), tagged `[web:backend]`.
Backend chain, first non-empty wins: Tavily (`TAVILY_API_KEY`) →
Brave (`BRAVE_API_KEY`) → DuckDuckGo (free) → Wikipedia FR+EN
(free, always available). No key required: free backends work out
of the box, premium keys just rank first. Optimized: 8 s per backend,
20 s deadline, fetch skipped when snippets suffice, query-relevant
passages, fair per-source budgets, 10 min cache. Works disconnected
too (raw results, zero tokens); the provider history is untouched
(display + ledger only, like agents).

## AGENTS.md (system context)

The binary reads `./AGENTS.md` then `./AGENT.md` from the cwd
(`--agents-file <file>` wins over both), once, at the first send
(never re-read; `/usage` says `missing` until then). Injected as a
`system` message atop the history of every chat (new ones included,
`/new` and Ctrl+N) and prefixed to `/agent` calls. Excluded from
`/compact` (meta summary) and from `--once` (raw). First-send
announcement (`AGENTS.md loaded,
N lines, ~M tokens`) and a dedicated `/usage` line (`loaded` or
`missing`).

## Modules (`src/`)

`main.rs` (CLI + bootstrap), `app.rs` (state + 100ms loop),
`mouse.rs` ([+] click = new chat, [×] click = close chat, edge drag =
sidebar width, click the roaming Kiro = hit him), `phantom.rs` +
`phantom/` (Kiro the critic: `motion.rs` wandering/knockback/KO,
`draw.rs` sprite + bubble + stars, `lines.rs` critique instruction +
retorts), `app/critic.rs` (critique calls, `/critic`, clicks),
`drain.rs` (routing async replies to chats),
`chat.rs` (history + input + maskable custom prompt),
`fx.rs` (horror overlay), `tokens.rs` (tiktoken counter),
`provider.rs` (OpenAI-compatible client, 60s timeout),
`sessions.rs` (multi-chats + sub-agents + `sessions/close.rs` closing),
`sidebar.rs` (coffins column + [+] new-chat button + [×] close),
`commands.rs` (parsing `/…` commands), `texts.rs` (help + usage texts),
`cmds.rs` (handlers `/model` `/thinking` `/agent` `/agents` `/ask` `/tell`
`/compact` `/connect` `/web` `/settings`),
`connect.rs` (`/connect` handlers + flow: masked key → model),
`settings.rs` + `settings/` (`/settings`: `known.rs` provider table,
`store.rs` encrypted vault, `ui.rs` screen, `keys.rs` key handling),
`web/` (`/web`: backends → fetch → cache → synthesis),
`tools/` (agent tools: `wire.rs` OpenAI types, `browser.rs`/`desktop.rs`
argv, `exec.rs` runner, `flow.rs` loop + y/n confirms, `log.rs` JSONL),
`agents.rs` (agents.md loading + tested pure compaction),
`app/ui.rs` (layout: header, panels, powerline bar),
`theme.rs` (single palette), `bestiary.rs` (sprites + integer
scaling), `gfx.rs` (Kitty / halfblock images), `apparition.rs`
(right-panel creature, `/bestiary` reveals),
`ghostty.rs` (OSC, progress, synced rendering),
`history.rs` (send window + chat naming),
`question.rs` (```question block: parse, render, digit reply),
`hub.rs` + `hub/` (spirit hub: tile-based simulation, `iso.rs`
isometric crypt, `render.rs` scene, `live.rs` per-frame Kitty image,
`font.rs` 3×5 pixel font, `social.rs` orbs and chatter),
`app/esprits.rs` (hub panel), `app/keys.rs` (dispatch + scroll + resize),
`shop.rs` (F6 shop: wallet, 1 token = 1 coin, 5 ambiance perks),
`canvas.rs` (shared RGBA painting: theme pixels, sprite blit with
alpha / tint / clip), `haunt.rs` + `haunt/` (F9 game: `level.rs` text
grid, `body.rs` tile physics, `player.rs` Kiro, `foes.rs` creatures,
`boss.rs` Reaper, `shots.rs` tokens and loot, `combat.rs`, `spawn.rs`
markers + director, `backdrop.rs` / `draw.rs` / `actors.rs` / `hud.rs`
rendering, `demo.rs` `--haunt-shot`), `app/haunt.rs` (game screen,
keys, Kitty keyboard protocol),
`almanac.rs` (tokens per day, `/usage` block) + `almanac/disk.rs`
(JSON file).
