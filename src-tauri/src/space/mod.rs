//! « Project space » : fond d'écran 3D des projets (DESIGN project-space).
//!
//! - [`scan`] : lecture de l'arbre des projets depuis le disque (§9),
//!   fonctions pures + tests ;
//! - [`events`] : `SceneEvent`, miroir Rust exact de `src/lib/space/types.ts`
//!   (serde `tag = "type"`, snake_case) + `SpaceLog` (canal Tauri
//!   `space-event`, désactivé tant qu'aucune `AppHandle` n'est branchée) ;
//! - [`cmds`] : commandes Tauri `space_scan` / `space_projects`.
//!
//! Aucun événement n'est émis pour l'instant : le branchement des sites
//! d'émission (chat, outils, kiro, hub) est une tranche ultérieure.

pub mod cmds;
pub mod events;
pub mod scan;
