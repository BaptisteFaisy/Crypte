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

## Limits

- Sessions live in RAM: tmux survives SSH drops, not VPS reboots.
  The JSONL stream is an observe-only replay, not a state restore.
- The TUI needs a terminal ≥ 80x24 (`tmux attach` provides it);
  headless modes (`--smoke`, `--once`, `--preview`) need none.
