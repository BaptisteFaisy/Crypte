//! kiro-horror-tui : chat LLM dans une crypte hantee.
//!
//! CLI : `--help`, `--smoke` (rendu headless sans reseau ni cle API),
//! `--config <fichier.toml>` (config provider, surchargeable par env),
//! `--once <texte>` (un appel puis exit), `--muse-auth` (cle depuis le store Muse),
//! `--agents-file <fichier>` (contexte systeme, prime sur ./AGENTS.md puis ./AGENT.md),
//! `--planche <png>` (bestiaire en PNG), `--apercu <fichier>` (frames en ANSI).

mod agents;
mod app;
mod apparition;
mod bestiary;
mod chat;
mod cmds;
mod commands;
mod drain;
mod fx;
mod galerie;
mod gfx;
mod ghostty;
mod mascot;
mod provider;
mod sessions;
mod theme;
mod tokens;

use anyhow::{Context, Result};

use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

use crate::app::{App, Screen};
use crate::chat::Role;
use crate::provider::{Provider, ProviderConfig};

const HELP: &str = "\
kiro-horror-tui 0.1.0 — chat LLM dans une crypte hantee

USAGE:
    kiro-horror-tui [OPTIONS]

OPTIONS:
    -h, --help            Affiche cette aide
        --smoke           Rendu headless de controle (80x24, sans reseau ni cle)
        --config <fichier>  Fichier TOML provider (cles plates ou [provider])
        --once <texte>      Un appel user puis exit (affiche texte + usage)
        --muse-auth         Cle depuis le store Muse (voir MUSE)
        --agents-file <fichier>  Contexte systeme (voir AGENTS.MD)
        --planche <png>     Ecrit la planche du bestiaire (34 dessins) en PNG
        --apercu <fichier>  Ecrit les 3 ecrans (120x36, hors ligne) en ANSI

CONFIG (environnement, prioritaire sur --config) :
    KH_BASE_URL   Base OpenAI-compatible (defaut https://api.openai.com/v1)
    KH_API_KEY    Cle API (obligatoire, jamais logguee)
    KH_MODEL      Modele (obligatoire)

MUSE (--muse-auth) :
    Store : ~/.config/muse/auth.json (cle : /providers/meta/api_key)
    Defauts : base https://api.meta.ai/v1, modele muse-spark-1.3-contributor
    (KH_BASE_URL / KH_API_KEY / KH_MODEL restent prioritaires)

AGENTS.MD (contexte systeme, charge une fois — restart pour recharger) :
    Lookup : --agents-file <f> prime, sinon ./AGENTS.md puis ./AGENT.md
    Injecte en tete de chaque chat + appels /agent (voir /usage)
    Exclu de /compact (resume meta) et de --once (brut)

TOUCHES (terminal >= 80x24) :
    Entree  envoyer    Echap/q  quitter    F2  grimoire (tokens)    F5  effroi
    F3/F4  chat precedent/suivant    Ctrl+N  nouveau chat    F6  bestiaire

GHOSTTY :
    Images via le protocole Kitty (demi-blocs ailleurs), fond de fenetre,
    titre, progression native, rendu synchronise. Theme + shader : ghostty/

COMMANDES (dans le chat) :
    /usage  tokens (chat + total)    /new  nouveau chat    /help  aide
    /thinking [niveau]  reflexion (default/low/medium/high)
    /model [nom|reset]  modele du chat (reset = defaut)
    /agent [@<n>] <texte>  sous-agent (reponse au chat n si cible)
    /agents  liste agents    /ask #<id> <texte>  reparler a un agent
    /tell <n> <texte>  parler au chat n
    /compact  compacter l'historique (resume + 4 derniers)
";

/// Options CLI parsees (sans dependance externe).
struct Args {
    help: bool,
    smoke: bool,
    config: Option<String>,
    once: Option<String>,
    muse_auth: bool,
    agents_file: Option<String>,
    planche: Option<String>,
    apercu: Option<String>,
}

/// Parse `std::env::args` : `--help`/`-h`, `--smoke`, `--config`, `--once`,
/// `--muse-auth`, `--agents-file`.
fn parse_args() -> Result<Args> {
    let mut args = Args {
        help: false,
        smoke: false,
        config: None,
        once: None,
        muse_auth: false,
        agents_file: None,
        planche: None,
        apercu: None,
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => args.help = true,
            "--smoke" => args.smoke = true,
            "--config" => {
                let path = it.next().context("--config attend un chemin de fichier")?;
                args.config = Some(path);
            }
            "--once" => {
                let prompt = it.next().context("--once attend un texte de prompt")?;
                args.once = Some(prompt);
            }
            "--muse-auth" => args.muse_auth = true,
            "--agents-file" => {
                let path = it
                    .next()
                    .context("--agents-file attend un chemin de fichier")?;
                args.agents_file = Some(path);
            }
            "--apercu" => {
                let path = it.next().context("--apercu attend un chemin de fichier")?;
                args.apercu = Some(path);
            }
            "--planche" => {
                let path = it.next().context("--planche attend un chemin .png")?;
                args.planche = Some(path);
            }
            other => anyhow::bail!("option inconnue : {other} (voir --help)"),
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
    println!("{}", done.text);
    println!(
        "tokens: in={} out={} total={}",
        done.prompt_tokens,
        done.completion_tokens,
        done.prompt_tokens.saturating_add(done.completion_tokens)
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
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
        println!("planche du bestiaire : {path}");
        return Ok(());
    }
    if let Some(path) = args.apercu.as_deref() {
        return apercu(path);
    }
    let cfg = resolve_config(&args)?;
    if let Some(prompt) = args.once.as_deref() {
        return run_once(&cfg, prompt).await;
    }
    let agents_md = crate::agents::load(args.agents_file.as_deref());
    let mut app = App::from_config_with_agents(cfg, agents_md)?;
    let mut terminal = ratatui::init();
    let result = match app.attach_terminal() {
        Ok(()) => app.run(&mut terminal).await,
        Err(err) => Err(err),
    };
    let _ = crate::ghostty::leave();
    ratatui::restore();
    result
}

/// App hors ligne garnie pour `--smoke` / `--apercu` : deux messages,
/// Kiro effrayé, quelques ticks d'effets.
fn demo_app() -> Result<App> {
    let mut app = App::offline()?;
    let chat = &mut app.sessions.current_mut().chat;
    chat.push(Role::User, "smoke: qui hante cette crypte ?");
    chat.push(Role::Assistant, "smoke: le fantome de la CI, et ca saigne.");
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
    anyhow::ensure!(
        filled > 100,
        "frame de controle presque vide ({filled} cellules)"
    );
    println!("smoke OK : frame 80x24, {filled} cellules remplies");
    println!(
        "smoke OK : tokens estimate('hello')={}",
        crate::tokens::estimate("hello")
    );
    Ok(())
}

/// Écrit les 3 écrans (120x36, hors ligne) en ANSI truecolor dans `path` :
/// `cat <fichier>` dans Ghostty montre l'UI sans clé ni réseau.
fn apercu(path: &str) -> Result<()> {
    let mut app = demo_app()?;
    let mut out = String::new();
    for screen in [Screen::Chat, Screen::Tokens, Screen::Bestiary] {
        out.push_str(&to_ansi(&render(&mut app, screen, 120, 36)?));
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("écriture de {path}"))?;
    println!("apercu : {path} (3 ecrans 120x36)");
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
