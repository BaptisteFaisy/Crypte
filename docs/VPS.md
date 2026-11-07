# Crypte on a VPS (watch it live from home)

Run the TUI on a Linux VPS, observe chats + sub-agents in real time
from your local machine. SSH only — zero exposed ports.

## 1. Build

On the VPS (Rust ≥ 1.74 + a C linker; no OpenSSL needed, rustls;
no system CA store needed, roots are embedded):

```sh
git clone https://github.com/BaptisteFaisy/Crypte.git /opt/crypte
cd /opt/crypte
cargo build --release
./target/release/kiro-horror-tui --smoke   # works with no key, no network
```

Or build locally and copy the single binary over (`scp`) — runtime
needs nothing else (pixel assets and the tokenizer are embedded).

## 2. Configure

Environment file (prefer a plain API key — never `--muse-auth` on a
VPS, it needs a logged-in `muse` CLI on that machine):

```sh
# /etc/crypte/env (chmod 600, owned by the service user)
KH_BASE_URL=https://api.openai.com/v1
KH_API_KEY=...
KH_MODEL=...
KH_EVENTS_LOG=/var/log/crypte/events.jsonl
```

`KH_EVENTS_LOG` (or `--events-log <file>`, flag wins) enables the
real-time event stream: every chat message and every sub-agent
spawn/reply appended as one JSON line. Unset = disabled (zero writes).

System context: pass `--agents-file /opt/crypte/AGENTS.md`, or run
with `WorkingDirectory=/opt/crypte` (`./AGENTS.md` is picked up).

## 3. Run (tmux, survives SSH drops)

```sh
sudo mkdir -p /var/log/crypte && sudo chown "$USER" /var/log/crypte
tmux new-session -d -s crypte -c /opt/crypte \
  'set -a; . /etc/crypte/env; set +a; exec ./target/release/kiro-horror-tui'
```

Persist across reboots: install `deploy/crypte.service`
(systemd starts the tmux session, stops it on service stop):

```sh
sudo cp deploy/crypte.service /etc/systemd/system/crypte.service
sudo systemctl daemon-reload && sudo systemctl enable --now crypte
```

## 4. Watch live (from your machine)

Full TUI mirror, read-only (your terminal drives the keys, the VPS
renders — images degrade to halfblocks outside Kitty):

```sh
ssh -t <vps> 'tmux attach -r -t crypte'
```

Structured event stream (quasi real time, greppable, `jq`-able):

```sh
ssh <vps> 'tail -F /var/log/crypte/events.jsonl'
ssh <vps> 'tail -F /var/log/crypte/events.jsonl' | jq -r \
  'select(.type=="agent_done") | "[agent#\(.agent)] \(.text[0:120])"'
```

Interact (read-write attach): `ssh -t <vps> 'tmux attach -t crypte'`.

## 5. Events reference

One JSON object per line. `ts` = epoch seconds, `chat`/`owner`/
`deliver` = 1-based chat numbers (as in the sidebar), `agent` =
global sub-agent id. `started` opens every run (session delimiter).

| type | fields |
| --- | --- |
| `started` | `version` |
| `chat_created` | `chat`, `name` |
| `user_msg` | `chat`, `text` |
| `assistant_msg` | `chat`, `text`, `prompt_tokens`, `completion_tokens` |
| `assistant_error` | `chat`, `error` |
| `agent_spawned` | `agent`, `owner`, `deliver`, `prompt` |
| `agent_ask` | `agent`, `owner`, `deliver`, `text` |
| `agent_done` | `agent`, `owner`, `deliver`, `text`, `prompt_tokens`, `completion_tokens` |
| `agent_failed` | `agent`, `owner`, `deliver`, `error` |
| `compacted` | `chat`, `before`, `after` |
| `web_answer` | `chat`, `backend`, `text` |

## 6. Log rotation

The file is reopened on every event, so a plain rename works (no
`copytruncate`, no signal needed):

```sh
# /etc/logrotate.d/crypte
/var/log/crypte/events.jsonl {
    daily
    rotate 14
    compress
    missingok
    notifempty
    create 0644 crypte crypte
}
```

## 7. Remote pilot without SSH (`--serve`)

Headless HTTP server: drive chats (and `/agent`, `/ask`, …) and watch
everything live from a browser, no SSH session needed:

```sh
KH_WATCH_TOKEN="$(openssl rand -hex 24)" \
  ./target/release/kiro-horror-tui --serve 127.0.0.1:8931 \
  --events-log /var/log/crypte/events.jsonl
```

Then open `http://<vps>:8931/?token=<token>` (single page, zero
dependencies, token also accepted in the page field). No tmux needed:
`--serve` never touches a terminal — see `deploy/crypte-serve.service`
for a plain systemd unit (`Type=simple`).

Routes (all require `Authorization: Bearer <token>`):

| route | use |
| --- | --- |
| `GET /` | the watch/pilot page |
| `GET /snapshot` | full state JSON (chats, messages, agents, tokens) |
| `GET /live` | Server-Sent Events of the §5 JSON (`?token=` also accepted) |
| `POST /input` | `{"chat": N, "text": "..."}` — sends (or runs `/commands`) |

Security: bind loopback + `ssh -L 8931:127.0.0.1:8931 <vps>` for a
single admin, or a Tailnet/private IP + strong token for a small team.
Never expose `--serve` to the public internet without a TLS terminator
in front (no HTTPS here, token in clear HTTP).

## Limits

- Chats autosave to `~/.config/kiro-horror-tui/chats.json` and are
  restored on restart (TUI and `--serve` alike) — a reboot no longer
  wipes them. Do not run two instances on the same account (they would
  overwrite each other's file).
- The JSONL stream is an observe-only replay, not a state restore.
- The TUI needs a terminal ≥ 80x24 (`tmux attach` provides it);
  headless modes (`--smoke`, `--once`, `--preview`, `--serve`) need none.
