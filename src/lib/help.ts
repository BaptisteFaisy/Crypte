// help.ts — aide /help (miroir de texts.rs : 33 commandes + touches).
// Le backend restant identique, le texte du TUI est repris tel quel.

export const HELP_TEXT = `Commands:
  /usage — token usage (chat + session + all-time almanac)
  /new — new chat (resets the current chat)
  /clear — clear the current chat conversation
  /bestiary — show a random creature in the side panel
  /thinking [level] — reasoning level (default/low/medium/high)
  /model [name|reset] — show or change the model
  /agent [@<n>] <text> — sub-agent (reply goes to chat n if targeted)
  /agents [all] — list sub-agents (all = every chat)
  /ask #<id> <text> — follow up with an agent
  /tell <n> <text> — talk to chat n
  /msg <n|@n|#id|all> <text> — talk to a chat, an agent, all
  /link <a> <b> — replies from a go to b (auto)
  /links — list auto links
  /unlink <a> <b> — remove a link
  /compact — compact the current chat history
  /connect [openai|muse] — connect (status if no argument)
  /settings [name] — providers screen: encrypted API keys
  /settings add <name> <base_url> [model] — custom provider
  /permissions [yolo|auto|<tool> <rule>|list|reset] — tools mode + per-tool rules (remembered)
  /web <query> — web search + summary with cited sources
  /tools [on|off] — model tools: browser, desktop, shell (default on)
  /kiro — kiro .kiro/ status (steering, specs, hooks, mcp)
  /spec <name> [run] | /spec new <name> <idea> — kiro spec workflow
  /mcp [refresh] — MCP servers: status, tools (refresh reconnects)
  /hooks — kiro hooks by trigger + recent firings
  /skills — list reusable skills (name + description)
  /skill <name> [text] — inject a skill atop the next send
  /checkpoints — list file snapshots (newest first)
  /rewind <n|name> — restore a checkpoint
  /remember <text> — memorize into .kiro/MEMORY.md
  /critic [on|off] — Kiro roams and judges your prompts (click to hit him)
  /vps [ip|name|off|status|add|del|install] — run this TUI on a VPS
  /help — this help
Questions: the AI may ask you — reply with the number or in your own words
Keys: F5 scare · F6 shop · F9 haunt (game) · Ctrl+N · Ctrl+W close (+ click) · Ctrl+C stop
Shop (F6 or tab click): 1 token consumed = 1 coin, ambiance perks
Context: ./AGENTS.md loaded at startup (restart to reload)`;
