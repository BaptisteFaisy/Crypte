//! Boutique de la crypte : les tokens consommés deviennent des pièces.
//!
//! Port pur de `shop.rs` du TUI (sans `ratatui`/`crossterm` ni écran) :
//! 1 token consommé (tous chats et sous-agents inclus) = 1 pièce,
//! créditée dans le portefeuille pour la session en cours. La boutique
//! vend des bonus d'ambiance (peur, apparitions, éclair) payés en pièces.
//! Le frontend rend depuis les vues (`cmd_shop`) ; l'achat par nom
//! (`buy_by_name`) remplace la sélection d'écran (`buy_selected` gardé).

/// Effet d'ambiance appliqué à l'achat d'un article.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopEffect {
    /// La peur chute de `n` (plancher 0).
    Calm(u8),
    /// Kiro s'effraie : éclair + apparition surprise.
    Scare,
    /// Une nouvelle créature apparaît au panneau droit.
    Apparition,
    /// Éclair seul, sans effroi.
    Flash,
}

/// Un article : nom, description, prix en pièces, effet.
pub struct ShopItem {
    /// Nom affiché.
    pub name: &'static str,
    /// Description affichée sous le nom.
    pub lore: &'static str,
    /// Prix en pièces (1 token consommé = 1 pièce).
    pub price: u64,
    /// Effet appliqué à l'achat.
    pub effect: ShopEffect,
}

/// Catalogue : 5 bonus d'ambiance, prix croissants.
pub const SHOP: &[ShopItem] = &[
    ShopItem {
        name: "Lightning flash",
        lore: "a flash with no scare",
        price: 200,
        effect: ShopEffect::Flash,
    },
    ShopItem {
        name: "Soothing candle",
        lore: "fear drops by 30",
        price: 300,
        effect: ShopEffect::Calm(30),
    },
    ShopItem {
        name: "Kiro's cry",
        lore: "Kiro panics: flash + apparition",
        price: 500,
        effect: ShopEffect::Scare,
    },
    ShopItem {
        name: "Apparition rite",
        lore: "a new creature emerges",
        price: 700,
        effect: ShopEffect::Apparition,
    },
    ShopItem {
        name: "Holy water",
        lore: "fear drops to zero",
        price: 900,
        effect: ShopEffect::Calm(100),
    },
];

/// Portefeuille : les pièces gagnées valent les tokens consommés de la
/// session, `spent` cumule les pièces dépensées. Solde = gagnées − dépensées.
#[derive(Debug, Default)]
pub struct Wallet {
    spent: u64,
}

impl Wallet {
    /// Portefeuille vide (rien dépensé).
    pub fn new() -> Self {
        Self::default()
    }

    /// Pièces dépensées cette session.
    pub fn spent(&self) -> u64 {
        self.spent
    }

    /// Solde pour `earned` pièces gagnées (jamais négatif).
    pub fn balance(&self, earned: u64) -> u64 {
        earned.saturating_sub(self.spent)
    }

    /// Paie `price` si le solde suffit (rend vrai si débité).
    pub fn spend(&mut self, earned: u64, price: u64) -> bool {
        if self.balance(earned) < price {
            return false;
        }
        self.spent = self.spent.saturating_add(price);
        true
    }
}

/// État de la boutique : portefeuille + message (la sélection d'écran
/// n'existe plus : l'achat se fait par nom, cf. `buy_by_name`).
#[derive(Debug, Default)]
pub struct Shop {
    wallet: Wallet,
    message: String,
}

impl Shop {
    /// Boutique neuve, portefeuille vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Portefeuille (solde via `balance`, dépensé via `spent`).
    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    /// Dernier message d'achat (vide = aucun achat tenté).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Achète l'article `name` : débite si solvable et rend l'effet à
    /// appliquer (`None` = trop cher ; erreur = article inconnu).
    pub fn buy_by_name(&mut self, earned: u64, name: &str) -> Result<Option<ShopEffect>, String> {
        let item = SHOP
            .iter()
            .find(|item| item.name == name)
            .ok_or_else(|| format!("unknown item: {name}"))?;
        if self.wallet.spend(earned, item.price) {
            self.message = format!("{} acquired (−{} coins)", item.name, item.price);
            Ok(Some(item.effect))
        } else {
            let missing = item.price - self.wallet.balance(earned);
            self.message = format!("not enough coins (missing {missing})");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn achete_debite_et_message() {
        let mut shop = Shop::new();
        assert_eq!(
            shop.buy_by_name(1000, "Lightning flash").unwrap(),
            Some(ShopEffect::Flash)
        );
        assert_eq!(shop.wallet().spent(), 200);
        assert_eq!(shop.wallet().balance(1000), 800);
        assert!(shop.message().contains("acquired"));
    }

    #[test]
    fn trop_cher_et_inconnu() {
        let mut shop = Shop::new();
        assert_eq!(shop.buy_by_name(10, "Holy water").unwrap(), None);
        assert!(shop.message().contains("missing 890"));
        assert!(shop.buy_by_name(1000, "zzz").is_err());
        assert_eq!(shop.wallet().spent(), 0);
    }
}
