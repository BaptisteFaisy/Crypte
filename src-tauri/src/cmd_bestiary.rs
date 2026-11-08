//! Commandes bestiaire §7 (partiel, `commands.md`) : get, random, list.
//!
//! `shop_state`/`shop_buy` (tranche fun, `shop.rs`), `apparition_state`/
//! `spook` (tranche fun, `apparition.rs`/`fx.rs`) ne sont pas ici.

use crate::bestiary;
use crate::core::Core;
use crate::dto::{CreatureDto, CreatureEntry};

impl Core {
    /// `bestiary_get` : créature par slug (bestiaire + Kiro), grille 32×32.
    pub fn bestiary_get(&self, slug: String) -> Result<CreatureDto, String> {
        bestiary::find(slug.trim())
            .map(dto_of)
            .ok_or_else(|| format!("unknown creature: {slug}"))
    }

    /// `bestiary_random` : créature au hasard dans les 33 (`/bestiary`).
    pub fn bestiary_random(&self) -> CreatureDto {
        dto_of(bestiary::random())
    }

    /// `bestiary_list` : catalogue des 33 (sans grilles).
    pub fn bestiary_list(&self) -> Vec<CreatureEntry> {
        bestiary::BESTIARY
            .iter()
            .map(|c| CreatureEntry {
                slug: c.slug.to_string(),
                name: c.name.to_string(),
                lore: c.lore.to_string(),
            })
            .collect()
    }
}

/// DTO complet d'une créature.
fn dto_of(creature: &bestiary::Creature) -> CreatureDto {
    CreatureDto {
        slug: creature.slug.to_string(),
        name: creature.name.to_string(),
        lore: creature.lore.to_string(),
        grid: creature.grid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tests::test_core;

    #[test]
    fn get_random_list() {
        let (core, _channels, dir) = test_core("bestiary");
        let kiro = core.bestiary_get(String::from("fantome")).unwrap();
        assert_eq!(kiro.name, "Kiro");
        assert_eq!(kiro.grid.len(), 32);
        assert!(core.bestiary_get(String::from("unicorn")).is_err());
        // Kiro hors bestiaire trouvable par get, pas dans la liste.
        assert!(core.bestiary_get(String::from("kiro-ko")).is_ok());
        let list = core.bestiary_list();
        assert_eq!(list.len(), 33);
        assert!(!list.iter().any(|e| e.slug == "kiro-ko"));
        let one = core.bestiary_random();
        assert_eq!(one.grid.len(), 32);
        assert!(bestiary::BESTIARY.iter().any(|c| c.slug == one.slug));
        std::fs::remove_dir_all(dir).ok();
    }
}
