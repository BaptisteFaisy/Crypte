//! Options CLI du binaire (extrait de `main.rs`, budget 500 lignes).
//!
//! `--help`, `--smoke`, `--demo`, `--config`, `--once`, `--muse-auth`,
//! `--agents-file`, `--planche`/`--sheet`, `--apercu`/`--preview`,
//! `--haunt-shot`, `--events-log`, `--mbox <cmd...>` (commande collectée
//! par `crate::mbox::outils`, tout le reste d'argv, `--` inclus).

use std::iter::Peekable;

use anyhow::{Context, Result};

/// Options CLI parsees (sans dependance externe).
pub(crate) struct Args {
    pub(crate) help: bool,
    pub(crate) smoke: bool,
    pub(crate) demo: bool,
    pub(crate) config: Option<String>,
    pub(crate) once: Option<String>,
    pub(crate) muse_auth: bool,
    pub(crate) agents_file: Option<String>,
    pub(crate) planche: Option<String>,
    pub(crate) apercu: Option<String>,
    pub(crate) haunt_shot: Option<String>,
    /// Planche PNG du monde 3D (`--space-shot`).
    pub(crate) space_shot: Option<String>,
    /// Dossier où écrire les frames d'une animation du monde 3D (`--space-anim`).
    pub(crate) space_anim: Option<String>,
    pub(crate) events_log: Option<String>,
    pub(crate) mbox: Option<Vec<String>>,
}

/// Parse `std::env::args`, une option à la fois ([`parse_option`]).
pub(crate) fn parse_args() -> Result<Args> {
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
        haunt_shot: None,
        space_shot: None,
        space_anim: None,
        events_log: None,
        mbox: None,
    };
    let mut it = std::env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        parse_option(&mut args, &mut it, &arg)?;
    }
    Ok(args)
}

/// Une option : drapeau stocké, valeur attendue, ou `--mbox` qui avale
/// le reste d'argv (collecté côté `mbox`, jamais une option du binaire).
fn parse_option(
    args: &mut Args,
    it: &mut Peekable<impl Iterator<Item = String>>,
    arg: &str,
) -> Result<()> {
    match arg {
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
        "--haunt-shot" => {
            let path = it.next().context("--haunt-shot expects a .png path")?;
            args.haunt_shot = Some(path);
        }
        "--space-shot" => {
            let path = it.next().context("--space-shot expects a .png path")?;
            args.space_shot = Some(path);
        }
        "--space-anim" => {
            let path = it.next().context("--space-anim expects a directory")?;
            args.space_anim = Some(path);
        }
        "--planche" | "--sheet" => {
            let path = it.next().context("--sheet expects a .png path")?;
            args.planche = Some(path);
        }
        "--events-log" => {
            let path = it.next().context("--events-log expects a file path")?;
            args.events_log = Some(path);
        }
        "--mbox" => args.mbox = Some(crate::mbox::outils::prend_commande_mbox(it.by_ref())),
        other => anyhow::bail!("unknown option: {other} (see --help)"),
    }
    Ok(())
}
