//! kiro-horror-tui : chat LLM dans une crypte hantee.
//!
//! CLI : `--help`, `--smoke` (rendu headless sans reseau ni cle API),
//! `--config <fichier.toml>` (config provider, surchargeable par env),
//! `--once <texte>` (un appel puis exit), `--muse-auth` (cle depuis le store Muse),
//! `--agents-file <fichier>` (contexte systeme, prime sur ./AGENTS.md puis ./AGENT.md, lu au premier envoi),
//! `--planche <png>` (bestiaire en PNG), `--apercu <fichier>` (frames en ANSI).

mod agents;
mod almanac;
mod app;
mod apparition;
mod bestiary;
mod bus;
mod chat;
mod cmds;
mod cmds_web;
mod commands;
mod connect;
mod drain;
mod events;
mod fx;
mod galerie;
mod gfx;
mod ghostty;
mod history;
mod hub;
mod mascot;
mod mouse;
mod permissions;
mod persist;
mod pool;
mod provider;
mod sessions;
mod sidebar;
mod theme;
mod tokens;
mod web;
mod workspace;

use std::{io, path::PathBuf};

use anyhow::{Context, Result};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

use crate::almanac::Almanac;
use crate::app::{App, Screen};
use crate::chat::Role;
use crate::events::{Event, EventLog};
use crate::provider::{Provider, ProviderConfig};

const HELP: &str = "\
kiro-horror-tui 0.1.0 — LLM chat in a haunted crypt

USAGE:
    kiro-horror-tui [OPTIONS]

OPTIONS:
    -h, --help            Show this help
        --smoke           Headless check render (80x24, no network, no key)
        --config <file>   Provider TOML file (flat keys or [provider])
        --once <text>     One user call then exit (prints text + usage)
        --muse-auth       Key from the Muse store (see MUSE)
        --agents-file <file>  System context (see AGENTS.MD)
        --sheet <png>     Write the bestiary sheet (34 drawings) as PNG
        --preview <file>  Write the 4 screens (120x36, offline) as ANSI
        --demo            Offline demo crypt, opened on the spirit hub (F7)
        --events-log <file>  Append chats+agents events as JSONL (see docs/VPS.md)

CONFIG (environment, wins over --config):
    KH_BASE_URL   OpenAI-compatible base (default https://api.openai.com/v1)
    KH_API_KEY    API key (never logged; --once requires it)
    KH_MODEL      Model (--once requires it)
    KH_CONTEXT_WINDOW  Context window override (tokens, integer > 0)
    KH_EVENTS_LOG   Same as --events-log (flag wins when both are set)
    BRAVE_API_KEY / TAVILY_API_KEY  Optional premium /web backends (else free)
    With no key, no --config and no --muse-auth, the TUI still starts,
    disconnected — /connect openai|muse to connect from inside.

MUSE (--muse-auth):
    Store: ~/.config/muse/auth.json (key: /providers/meta/api_key)
    Defaults: base https://api.meta.ai/v1, model muse-spark-1.3-contributor
    (KH_BASE_URL / KH_API_KEY / KH_MODEL take priority)

AGENTS.MD (system context, read once at first send):
    Lookup: --agents-file <f> wins, else ./AGENTS.md then ./AGENT.md
    Pinned atop every chat + /agent calls (see /usage)
    Excluded from /compact (meta summary) and from --once (raw)

KEYS (terminal >= 80x24):
    Enter  send    q  quit    F5  scare
    F3/F4  previous/next chat    Ctrl+N or click [+]  new chat    F6  bestiary    F7  hub
    F8  almanac (tokens per day, kept across sessions)

ALMANAC (tokens per day, local date):
    File: $XDG_DATA_HOME/kiro-horror-tui/almanac.json
          (else ~/.local/share/kiro-horror-tui/almanac.json)
    Every billed call counts (chats, agents, /compact, /web, --once)

GHOSTTY:
    Images via the Kitty protocol (halfblocks elsewhere), window
    background, title, native progress, synced rendering. Theme + shader:
    ghostty/

COMMANDS (in chat):
    /usage  tokens (chat + total)    /new  new chat    /help  help
    /thinking [level]  reasoning (default/low/medium/high)
    /model [name|reset]  chat model (reset = default)
    /agent [@<n>] <text>  sub-agent (reply goes to chat n if targeted)
    /agents [all]  list agents    /ask #<id> <text>  follow up with an agent
    /tell <n> <text>  talk to chat n (queued if busy)
    /msg <n|@n|#id|all> <text>  talk to a chat, an agent, all
    /link <a> <b>  replies from a go to b (auto)    /links  list
    /unlink <a> <b>  remove a link
    /web <query>  web search + cited summary (raw results if offline)
    /compact  compact history (summary + last 4)
    /connect [openai|muse]  interactive connect (masked key,
        Esc cancels); no arg: connection status
    /permissions [yolo|auto]  future tools mode (remembered)
    context limit: model window capped at 1M est. tokens (see /usage)
";

/// Options CLI parsees (sans dependance externe).
struct Args {
    help: bool,
    smoke: bool,
    demo: bool,
    config: Option<String>,
    once: Option<String>,
    muse_auth: bool,
    agents_file: Option<String>,
    planche: Option<String>,
    apercu: Option<String>,
    events_log: Option<String>,
}

/// Parse `std::env::args` : `--help`/`-h`, `--smoke`, `--config`, `--once`,
/// `--muse-auth`, `--agents-file`.
fn parse_args() -> Result<Args> {
    let mut args = Args {
        help: false,
        smoke: false,
        demo: false,
        config: None,
        once: None,
        muse_auth: false,
        agents_file: None,
        planche: None,
        apercu: None,
        events_log: None,
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => args.help = true,
            "--smoke" => args.smoke = true,
            "--demo" => args.demo = true,
            "--config" => {
                let path = it.next().context("--config expects a file path")?;
                args.config = Some(path);
            }
            "--once" => {
                let prompt = it.next().context("--once expects a prompt text")?;
                args.once = Some(prompt);
            }
            "--muse-auth" => args.muse_auth = true,
            "--agents-file" => {
                let path = it.next().context("--agents-file expects a file path")?;
                args.agents_file = Some(path);
            }
            "--apercu" | "--preview" => {
                let path = it.next().context("--preview expects a file path")?;
                args.apercu = Some(path);
            }
            "--planche" | "--sheet" => {
                let path = it.next().context("--sheet expects a .png path")?;
                args.planche = Some(path);
            }
            "--events-log" => {
                let path = it.next().context("--events-log expects a file path")?;
                args.events_log = Some(path);
            }
            other => anyhow::bail!("unknown option: {other} (see --help)"),
        }
    }
    Ok(args)
}

/// Resout la config : `--muse-auth` > `--config` > environnement.
fn resolve_config(args: &Args) -> Result<ProviderConfig> {
    if args.muse_auth {
        let path = ProviderConfig::muse_auth_path()?;
        return ProviderConfig::from_muse_auth(path);
    }
    match args.config.as_deref() {
        Some(path) => ProviderConfig::from_toml(path),
        None => ProviderConfig::from_env(),
    }
}

/// Un appel user puis exit : affiche le texte puis la ligne d'usage.
async fn run_once(cfg: &ProviderConfig, prompt: &str) -> Result<()> {
    let provider = Provider::new(cfg.clone())?;
    let done = provider
        .complete(&[("user".to_string(), prompt.to_string())])
        .await?;
    let mut almanac = Almanac::open_default();
    almanac.record(done.prompt_tokens, done.completion_tokens);
    if let Some(err) = almanac.error() {
        eprintln!("almanac: {err}");
    }
    println!("{}", done.text);
    println!(
        "tokens: in={} out={} total={}",
        done.prompt_tokens,
        done.completion_tokens,
        done.prompt_tokens.saturating_add(done.completion_tokens)
    );
    Ok(())
}

// Runtime resserré : la boucle TUI bloque un worker dans `event::poll`
// (jamais de `.await`), 2 workers suffisent ; les tâches ne font que de
// l'IO peu profonde (un POST reqwest + serde), 1 Mio de pile suffit.
// Le pool bloquant reste paresseux (0 thread au repos, aucun usage).
fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(1024 * 1024)
        .enable_all()
        .build()
        .context("construction du runtime tokio")?
        .block_on(async_main())
}

async fn async_main() -> Result<()> {
    let args = parse_args()?;
    if args.help {
        print!("{HELP}");
        return Ok(());
    }
    if args.smoke {
        return smoke();
    }
    if let Some(path) = args.planche.as_deref() {
        crate::bestiary::write_sheet(path)?;
        println!("bestiary sheet: {path}");
        return Ok(());
    }
    if let Some(path) = args.apercu.as_deref() {
        return apercu(path);
    }
    if let Some(prompt) = args.once.as_deref() {
        let cfg = resolve_config(&args)?;
        return run_once(&cfg, prompt).await;
    }
    let mut app = match resolve_config(&args) {
        _ if args.demo => App::offline()?,
        Ok(cfg) => App::from_config_with_agents_file(cfg, args.agents_file)?,
        Err(_) => App::disconnected_with_agents_file(args.agents_file),
    };
    if !args.demo {
        app.enable_history();
        app.almanac = Almanac::open_default();
    }
    let events = EventLog::new(
        args.events_log
            .clone()
            .or_else(|| std::env::var("KH_EVENTS_LOG").ok())
            .map(PathBuf::from),
    );
    events.emit(&Event::started());
    app.events = events;
    if args.demo {
        app.almanac = Almanac::demo(crate::almanac::today());
        app.populate_hub_demo();
        app.open_hub();
    }
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    let result = match app.attach_terminal() {
        Ok(()) => app.run(&mut terminal).await,
        Err(err) => Err(err),
    };
    let _ = crate::ghostty::leave();
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

/// App hors ligne garnie pour `--smoke` / `--apercu` : deux messages,
/// almanach de démo (mémoire), Kiro effrayé, quelques ticks d'effets.
fn demo_app() -> Result<App> {
    let mut app = App::offline()?;
    app.almanac = Almanac::demo(crate::almanac::today());
    let chat = &mut app.sessions.current_mut().chat;
    chat.push(Role::User, "smoke: who haunts this crypt?");
    chat.push(Role::Assistant, "smoke: the CI ghost, and it bleeds.");
    app.populate_hub_demo();
    for _ in 0..60 {
        app.tick();
    }
    app.spook();
    for _ in 0..5 {
        app.tick();
    }
    Ok(app)
}

/// Rend une frame de `app` (écran `screen`) dans un buffer `w`x`h`.
fn render(app: &mut App, screen: Screen, w: u16, h: u16) -> Result<Buffer> {
    app.screen = screen;
    let mut term = Terminal::new(TestBackend::new(w, h))?;
    term.draw(|frame| app.draw(frame))?;
    Ok(term.backend().buffer().clone())
}

/// Controle headless : une frame 80x24 de l'UI complète (chat, Kiro,
/// apparition, effets), sans reseau ni cle API. Echec = `Err`.
fn smoke() -> Result<()> {
    let mut app = demo_app()?;
    let buf = render(&mut app, Screen::Chat, 80, 24)?;
    let filled = buf.content.iter().filter(|c| c.symbol() != " ").count();
    anyhow::ensure!(filled > 100, "check frame almost empty ({filled} cells)");
    println!("smoke OK: 80x24 frame, {filled} cells filled");
    let buf = render(&mut app, Screen::Almanac, 80, 24)?;
    let bars = buf.content.iter().filter(|c| c.symbol() == "█").count();
    anyhow::ensure!(bars > 0, "almanac frame has no bars");
    println!("smoke OK: almanac 80x24, {bars} bar cells");
    println!(
        "smoke OK: tokens estimate('hello')={}",
        crate::tokens::estimate("hello")
    );
    Ok(())
}

/// Écrit les 4 écrans (120x36, hors ligne) en ANSI truecolor dans `path` :
/// `cat <fichier>` dans Ghostty montre l'UI sans clé ni réseau.
fn apercu(path: &str) -> Result<()> {
    let mut app = demo_app()?;
    let mut out = String::new();
    for screen in [Screen::Chat, Screen::Bestiary, Screen::Hub, Screen::Almanac] {
        out.push_str(&to_ansi(&render(&mut app, screen, 120, 36)?));
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("writing {path}"))?;
    println!("preview: {path} (4 screens 120x36)");
    Ok(())
}

/// Buffer → texte ANSI truecolor (fond, texte, gras, italique).
fn to_ansi(buf: &Buffer) -> String {
    use ratatui::style::{Color, Modifier};
    let sgr = |c: Color, base: u8| match c {
        Color::Rgb(r, g, b) => format!("\x1b[{base};2;{r};{g};{b}m"),
        _ => format!("\x1b[{}m", base + 1),
    };
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            out.push_str("\x1b[0m");
            out.push_str(&sgr(cell.fg, 38));
            out.push_str(&sgr(cell.bg, 48));
            if cell.modifier.contains(Modifier::BOLD) {
                out.push_str("\x1b[1m");
            }
            if cell.modifier.contains(Modifier::ITALIC) {
                out.push_str("\x1b[3m");
            }
            out.push_str(cell.symbol());
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helps_ne_mentionnent_plus_le_grimoire() {
        for help in [HELP, crate::commands::help_text()] {
            assert!(!help.contains("grimoire"));
            assert!(!help.contains("F2"));
        }
    }
}
