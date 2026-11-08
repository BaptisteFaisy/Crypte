# Cloud sessions Kiro (`/cloud`)

Run Kiro agents in a managed cloud sandbox, from inside Crypte.
Crypte drives the `kiro-cli` headless contract
(`kiro.dev/docs/cli/headless`, `kiro.dev/docs/cloud-sessions`) —
it never talks to the Kiro backend directly.

## Prerequisites

- `kiro-cli` 2.17+ on `PATH` (install: `curl -fsSL
  https://cli.kiro.dev/install | bash`), logged in (`kiro-cli login`)
  or `KIRO_API_KEY` set for headless use.
- Cloud sessions enabled for the account (admin switch in the AWS
  console for Identity Center orgs) ; max 10 concurrent sessions.
- The sandbox clones the repo server-side: commit and push first —
  uncommitted or unpushed work is invisible in the cloud (Crypte
  warns, it doesn't block).

## Commands

- `/cloud` (or `/cloud status`) — CLI version, resolved repo, git
  state, running jobs, web link.
- `/cloud run [--repo owner/repo] <task>` — start a cloud agent.
  Repo defaults to the `origin` of the project dir. Progress
  streams as `[cloud#n]` lines (chat only, never history) ; the
  final note gives the session id, `kiro-cli --resume-id <id>`
  and `https://app.kiro.dev`. Runs use `--trust-all-tools`
  (cloud sessions have no per-change approval anyway).
- `/cloud list` — CLI session list (local + cloud, raw output).
- `/cloud attach <id>` — how to resume outside Crypte (terminal,
  web, mobile — same session everywhere).
- `/cloud delete <id>` — delete a session.
- `/cloud stop [#n]` — kill the local process (the cloud session
  keeps running ; its id is shown when known).

## Notes

- Cloud usage bills the Kiro account, not the Crypte provider :
  cloud runs add no tokens to the ledger or shop.
- One-shot queries (`list`, `delete`) time out after 60 s.
- Tests are hermetic : `KH_KIRO_CLI` overrides the binary (shell
  stub), `KH_KIRO_PATH` overrides the searched `PATH`.
