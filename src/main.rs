//! kiro-horror-tui : chat LLM dans une crypte hantee.
//!
//! CLI : `--help`, `--smoke` (rendu headless sans reseau ni cle API),
//! `--config <fichier.toml>` (config provider, surchargeable par env),
//! `--once <texte>` (un appel puis exit), `--muse-auth` (cle depuis le store Muse),
//! `--agents-file <fichier>` (contexte systeme, prime sur ./AGENTS.md puis ./AGENT.md).

mod agents;
mod app;
mod chat;
mod cmds;
mod commands;
mod drain;
mod fx;
mod mascot;
mod provider;
mod sessions;
mod tokens;

use anyhow::{Context, Result};

use crate::app::App;
use crate::chat::{ChatState, Role};
use crate::fx::Fx;
use crate::mascot::Mascot;
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
    Entree  envoyer    Echap/q  quitter    F2  ecran tokens    F5  spook
    F3/F4  chat precedent/suivant    Ctrl+N  nouveau chat

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
    let cfg = resolve_config(&args)?;
    if let Some(prompt) = args.once.as_deref() {
        return run_once(&cfg, prompt).await;
    }
    let agents_md = crate::agents::load(args.agents_file.as_deref());
    let mut app = App::from_config_with_agents(cfg, agents_md)?;
    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}

/// Controle headless : une frame 80x24 (chat + mascotte + overlay fx),
/// sans reseau ni cle API. Echec = panic ou `Err`, succes = `Ok(())`.
fn smoke() -> Result<()> {
    use ratatui::{backend::TestBackend, Terminal};

    let mut chat = ChatState::new();
    chat.push(Role::User, "smoke: qui hante cette crypte ?");
    chat.push(Role::Assistant, "smoke: le fantome de la CI, et ca saigne.");
    let mut mascot = Mascot::new();
    let mut fx = Fx::new();
    mascot.spook();
    for _ in 0..5 {
        mascot.tick();
        fx.tick(true);
    }
    let mut term = Terminal::new(TestBackend::new(80, 24))?;
    term.draw(|frame| {
        let area = frame.area();
        let cols = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Percentage(60),
                ratatui::layout::Constraint::Percentage(40),
            ])
            .split(area);
        chat.draw(frame, cols[0]);
        mascot.draw(frame, cols[1]);
        fx.draw(area, frame.buffer_mut());
    })?;
    let buf = term.backend().buffer().clone();
    let filled = buf.content.iter().filter(|c| c.symbol() != " ").count();
    anyhow::ensure!(
        filled > 100,
        "frame de controle presque vide ({filled} cellules)"
    );
    println!(
        "smoke OK : frame 80x24, {filled} cellules remplies, peur={}, tick={}",
        fx.fear(),
        fx.frame()
    );
    println!(
        "smoke OK : tokens estimate('hello')={}",
        crate::tokens::estimate("hello")
    );
    Ok(())
}
