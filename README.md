# Crypte (Tauri + Svelte)

LLM chat in a haunted crypt, as a desktop app: a bestiary of pixel-art
creatures, the spirit hub, Kiro the mascot ghost, apparitions that react
to the chat, horror effects (vignette, fog, lightning), a header **fear
gauge** (`fear`, % of the model's context window), and a **global** token
counter. Port of the `kiro-horror-tui` terminal app to **Tauri v2** (Rust
backend) + **Svelte 5** (web frontend), same features, same crypt palette.

Screens: Chat, Settings, Vps, Shop, Haunt (+ bestiary gallery), plus the
inter-agent panel (bus + mailbox).

## Stack

- Backend: Rust 2021, Tauri v2 (`src-tauri/`, lib `crypte_lib` + `crypte` bin)
- Frontend: SvelteKit 2 in static SPA mode (`ssr=false`), Svelte 5 runes,
  Vite, TypeScript strict, Vitest
- Bridge: ~75 `#[tauri::command]`s + 2 event channels (`crypte-event`,
  `crypte-ui`) — contracts in `src/lib/contracts.ts`

## Develop

Prerequisites: Rust stable, Node 22+, Tauri v2 system libs
(`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
librsvg2-dev` on Debian).

```sh
npm install
npm run tauri dev   # desktop app with hot reload
npm run check       # svelte-check (types)
npm run test        # vitest
npm run build       # static frontend -> build/
cd src-tauri && cargo test   # backend tests (447)
```

Gates (must stay green): `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings`, `cargo test`, `npm run check`,
`npm test`, `npm run build`.

## Headless CLI

The binary embeds the TUI's headless surface (same names, same effects):

```sh
crypte --smoke            # self-check, no network, no key
crypte --sheet out.png    # bestiary sheet (34 drawings) as PNG
crypte --apercu out.json  # preview report as JSON
crypte --mbox <args...>   # broadcast CLI (same files as bcast shell)
crypte --events-log f     # append domain events as JSONL (KH_EVENTS_LOG)
```

## Data & migration

New paths under the Tauri app id (`com.crypte.app`):

- config: `$XDG_CONFIG_HOME/com.crypte.app` (`chats.json`, `permissions`,
  `permissions-rules`, `vps.toml`)
- data: `$XDG_DATA_HOME/com.crypte.app` (`providers.enc`, `master.key`,
  `almanac.json`, `checkpoints/`)

First launch auto-migrates (copy-once, never overwrites) from the legacy
TUI paths (`kiro-horror-tui`). Shared, never migrated: the mbox broadcast
dir (`BCAST_DIR`), `~/.config/muse/auth.json`, `~/.kiro/`.
`/vps` keeps the remote TUI in tmux (V1).

## Inter-agent communication

Two layers, both ported:

- **Bus** (in-process): chats and sub-agents talk (`/tell`, `/ask`,
  `/msg`, `/link`, `/unlink`, `/links`), queued delivery, 8-hop loop cap.
- **Mailbox** (broadcast v2 mirror): file-backed agent mailbox —
  P0/P1/P2 messages, `O-AAAAMMJJ-NNN` orders + acks, tokens, throttle,
  digest, 15 s probe, `--mbox` CLI and typed commands.

## Rules

- Files < 500 lines AND ≤ 30 functions; functions < 50 lines
  (`grep -cE 'fn [a-zA-Z_][a-zA-Z0-9_]* *\('`); zero duplication.
- Secrets never logged (manual redacted `Debug`s; `debug_redacted` tests).
- CI: every job ≤ 5 min (`ci-budget` guard); checks `fmt`, `clippy`,
  `build`, `test` required by the Mergify queue.
