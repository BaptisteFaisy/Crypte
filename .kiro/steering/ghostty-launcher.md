---
inclusion: manual
---

# Crypte — lanceur Ghostty et VPS

Référencer avec `#ghostty-launcher` quand on touche au lancement ou au VPS.

- `ghostty/crypte` : lance le TUI sur place si le terminal peut héberger la
  crypte (TTY interactif, truecolor, ≥ 80x24), sinon ouvre une fenêtre Ghostty
  habillée (thème + shader, build release si besoin). `--window` force la
  fenêtre ; `--help`, `--smoke`, `--once`, `--sheet`, `--preview` tournent
  toujours sur place. Si truecolor non annoncé : `export COLORTERM=truecolor`.
- `ghostty/crypte rebuild …` : synchronise le dernier `origin/main` dans un
  worktree dédié `<nom>-latest`, force un build release, puis le lance avec
  les args restants. Le checkout courant n'est jamais touché (arbre sale OK ;
  seul un worktree géré sale refuse). Ensuite, un lancement simple ouvre le
  build latest s'il existe, sinon celui du checkout.
- `ghostty/crypte.conf` (palette, padding étendu, 120x36) +
  `ghostty/crypte.glsl` (vignette statique — pas de blur : chaque pixel est
  lu à sa propre place). Capacités Ghostty utilisées : Kitty graphics,
  truecolor, sprites intégrés, OSC 11 / OSC 2 / OSC 9;4 (progression native
  pendant la simmer), mode 2026 (rendu synchronisé).
- VPS + flux d'événements live : #[[file:docs/VPS.md]] ; flux JSONL temps
  réel via `--events-log` / `KH_EVENTS_LOG` (chats + sous-agents, silencieux
  si désactivé). Profils VPS : `~/.config/kiro-horror-tui/vps.toml`.
