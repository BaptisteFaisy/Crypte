use super::*;

#[test]
fn wrap_breaks_on_words_and_cuts_long_ones() {
    assert_eq!(
        wrap_text("le chat noir dort", 8),
        ["le chat", "noir", "dort"]
    );
    assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap_text("a\n\nb", 10), ["a", "", "b"]);
    for w in 1..12 {
        for row in wrap_text("les ossements craquent sous la lune rousse", w) {
            assert!(row.chars().count() <= w, "{row:?} > {w}");
        }
    }
}

#[test]
fn heap_bytes_croit_avec_les_messages() {
    let mut chat = ChatState::new();
    let vide = chat.heap_bytes();
    chat.push(Role::User, "bonsoir");
    assert!(chat.heap_bytes() > vide);
    assert!(chat.heap_bytes() >= "bonsoir".len());
}

#[test]
fn defiler_affiche_les_vieux_messages_puis_revient() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut chat = ChatState::new();
    for i in 0..12 {
        chat.push(Role::User, format!("m{i:02}"));
    }
    let view = ChatView {
        title: "test",
        tick: 0,
        sending: false,
        question_pending: false,
    };
    let dump = |chat: &mut ChatState| {
        let mut term = Terminal::new(TestBackend::new(40, 14)).unwrap();
        term.draw(|f| chat.draw(f, f.area(), view)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..14)
            .map(|y| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    // Au bas : la fin visible, le début hors cadre, pas de badge.
    let bottom = dump(&mut chat);
    assert!(bottom.contains("m11"), "dernier message absent : {bottom}");
    assert!(!bottom.contains("m00"), "début affiché à tort : {bottom}");
    assert!(!bottom.contains("line"), "badge à tort : {bottom}");
    // Remonté tout en haut : le début réapparaît + badge de défilement.
    chat.scroll.by(-100);
    let top = dump(&mut chat);
    assert!(top.contains("m00"), "début toujours absent : {top}");
    assert!(!top.contains("m11"), "fin encore affichée : {top}");
    assert!(top.contains("lines"), "badge de défilement absent : {top}");
    // Retour au bas : on retrouve exactement la vue initiale.
    chat.scroll.to_bottom();
    assert_eq!(dump(&mut chat), bottom);
}

#[test]
fn history_has_one_tag_line_per_message() {
    let mut chat = ChatState::new();
    chat.push(Role::User, "bonsoir");
    chat.push(Role::Assistant, "bienvenue dans la crypte");
    let lines = history_lines(chat.messages(), 40);
    assert_eq!(lines.len(), 5);
}

#[test]
fn wrap_cache_ne_recalcule_que_si_messages_ou_largeur_changent() {
    let mut chat = ChatState::new();
    assert_eq!(chat.wrap_cache_key(), None);
    chat.push(Role::User, "bonsoir");
    chat.push(Role::Assistant, "bienvenue dans la crypte");
    let lines = chat.wrapped_lines(40).to_vec();
    assert_eq!(chat.wrap_cache_key(), Some((2, 40)));
    assert_eq!(lines, history_lines(chat.messages(), 40));
    // Second rendu à l'identique : hit, pas de recalcul.
    let _ = chat.wrapped_lines(40);
    assert_eq!(chat.wrap_cache_key(), Some((2, 40)));
    // Resize : recalcul, même version.
    let _ = chat.wrapped_lines(30);
    assert_eq!(chat.wrap_cache_key(), Some((2, 30)));
    // Nouveau message : stale jusqu'au prochain rendu.
    chat.push(Role::User, "encore");
    assert_eq!(chat.wrap_cache_key(), Some((2, 30)));
    let lines = chat.wrapped_lines(30).to_vec();
    assert_eq!(chat.wrap_cache_key(), Some((3, 30)));
    assert_eq!(lines, history_lines(chat.messages(), 30));
}

#[test]
fn prompt_masked_hides_key_length_only() {
    let mut chat = ChatState::new();
    chat.input.push_str("brouillon");
    chat.set_prompt(Some(String::from("Muse API key (masked)")), true);
    chat.set_input_preview(Some(String::from("sk-secret")));
    assert_eq!(chat.prompt_label.as_deref(), Some("Muse API key (masked)"));
    assert!(chat.mask_input);
    let shown = mask_text(
        chat.input_preview.as_ref().unwrap_or(&chat.input),
        chat.mask_input,
    );
    assert_eq!(shown, "•••••••••");
    assert!(!shown.contains("sk-secret"));
    assert_eq!(chat.input, "brouillon");
    chat.set_prompt(None, false);
    chat.set_input_preview(None);
    assert_eq!(chat.prompt_label, None);
    assert!(!chat.mask_input);
}

#[test]
fn curseur_saisie_fixe_ne_clignote_pas() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut chat = ChatState::new();
    chat.input.push_str("bonsoir");
    let dump = |chat: &mut ChatState, tick: u64| {
        let view = ChatView {
            title: "test",
            tick,
            sending: false,
            question_pending: true,
        };
        let mut term = Terminal::new(TestBackend::new(40, 14)).unwrap();
        term.draw(|f| chat.draw(f, f.area(), view)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..14)
            .map(|y| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    // Deux phases opposées de l'ancien clignotement : rendu identique.
    let a = dump(&mut chat, 0);
    let b = dump(&mut chat, 5);
    assert_eq!(a, b);
    assert!(a.contains('▌'), "curseur fixe absent : {a}");
}
