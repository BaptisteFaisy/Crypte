use super::*;

#[test]
fn parse_permissions_mots_suivants() {
    assert_eq!(parse("/permissions"), Some(Command::Permissions(vec![])));
    assert_eq!(
        parse("/permissions yolo"),
        Some(Command::Permissions(vec![String::from("yolo")]))
    );
    assert_eq!(
        parse("/permissions shell_run deny"),
        Some(Command::Permissions(vec![
            String::from("shell_run"),
            String::from("deny")
        ]))
    );
    assert!(crate::texts::help_text().contains("/permissions [yolo|auto|<tool>"));
}

#[test]
fn parse_skills_skill_checkpoints_rewind_remember() {
    assert_eq!(parse("/skills"), Some(Command::Skills));
    assert_eq!(parse("/checkpoints"), Some(Command::Checkpoints));
    assert_eq!(
        parse("/skill terse"),
        Some(Command::Skill {
            name: String::from("terse"),
            text: None,
        })
    );
    assert_eq!(
        parse("/skill terse hello you"),
        Some(Command::Skill {
            name: String::from("terse"),
            text: Some(String::from("hello you")),
        })
    );
    assert_eq!(parse("/skill"), Some(Command::Hint(SKILL_USAGE)));
    assert_eq!(
        parse("/rewind 2"),
        Some(Command::Rewind {
            target: String::from("2"),
        })
    );
    assert_eq!(parse("/rewind"), Some(Command::Hint(REWIND_USAGE)));
    assert_eq!(parse("/rewind a b"), Some(Command::Hint(REWIND_USAGE)));
    assert_eq!(
        parse("/remember likes terse"),
        Some(Command::Remember(String::from("likes terse")))
    );
    assert_eq!(parse("/remember"), Some(Command::Hint(REMEMBER_USAGE)));
    assert!(crate::texts::help_text().contains("/skill <name> [text]"));
    assert!(crate::texts::help_text().contains("/rewind <n|name>"));
    assert!(crate::texts::help_text().contains("/remember <text>"));
}

#[test]
fn parse_tools_marche_arret_optionnels() {
    assert_eq!(parse("/tools"), Some(Command::Tools(None)));
    assert_eq!(
        parse("/tools off"),
        Some(Command::Tools(Some(String::from("off"))))
    );
    assert!(crate::texts::help_text().contains("/tools [on|off]"));
}

#[test]
fn parse_critic_marche_arret_optionnels() {
    assert_eq!(parse("/critic"), Some(Command::Critic(None)));
    assert_eq!(
        parse("/critic off"),
        Some(Command::Critic(Some(String::from("off"))))
    );
    assert!(crate::texts::help_text().contains("/critic [on|off]"));
}

#[test]
fn parse_kiro_sans_argument() {
    assert_eq!(parse("/kiro"), Some(Command::Kiro));
    assert!(crate::texts::help_text().contains("/kiro"));
}

#[test]
fn parse_clear_sans_argument() {
    assert_eq!(parse("/clear"), Some(Command::Clear));
    assert_eq!(parse("  /clear x  "), Some(Command::Clear));
    assert!(crate::texts::help_text().contains("/clear"));
}

#[test]
fn parse_bestiary_sans_argument() {
    assert_eq!(parse("/bestiary"), Some(Command::Bestiary));
    assert_eq!(parse("  /bestiary x  "), Some(Command::Bestiary));
    assert!(crate::texts::help_text().contains("/bestiary"));
}

#[test]
fn parse_mcp_refresh_optionnel() {
    assert_eq!(parse("/mcp"), Some(Command::Mcp(None)));
    assert_eq!(
        parse("/mcp refresh"),
        Some(Command::Mcp(Some(String::from("refresh"))))
    );
    assert!(crate::texts::help_text().contains("/mcp [refresh]"));
}

#[test]
fn parse_hooks_sans_argument() {
    assert_eq!(parse("/hooks"), Some(Command::Hooks));
    assert!(crate::texts::help_text().contains("/hooks"));
}

#[test]
fn parse_matches_commands_first_word_only() {
    assert_eq!(parse("/usage"), Some(Command::Usage));
    assert_eq!(parse("  /new x  "), Some(Command::New));
    assert_eq!(parse("/help"), Some(Command::Help));
}

#[test]
fn parse_rejects_plain_text() {
    assert_eq!(parse("hello"), None);
    assert_eq!(parse("  bonjour /usage"), None);
    assert_eq!(parse(""), None);
}

#[test]
fn parse_unknown_keeps_word() {
    assert_eq!(parse("/zzz"), Some(Command::Unknown(String::from("/zzz"))));
    assert!(is_command("/zzz"));
    assert!(!is_command("hello"));
}

#[test]
fn parse_thinking_model_agent_agents_compact() {
    assert_eq!(
        parse("/thinking high"),
        Some(Command::Thinking(Some(String::from("high"))))
    );
    assert_eq!(parse("/thinking"), Some(Command::Thinking(None)));
    assert_eq!(parse("/model"), Some(Command::Model(None)));
    assert_eq!(
        parse("/agent txt"),
        Some(Command::Agent {
            target: None,
            prompt: String::from("txt")
        })
    );
    assert_eq!(
        parse("/agent   "),
        Some(Command::Unknown(String::from("/agent")))
    );
    assert_eq!(parse("/agents"), Some(Command::Agents { all: false }));
    assert_eq!(parse("/agents all"), Some(Command::Agents { all: true }));
    assert_eq!(parse("/agents x"), Some(Command::Agents { all: false }));
    assert_eq!(parse("/compact"), Some(Command::Compact));
    assert_eq!(parse("/connect"), Some(Command::Connect(None)));
    assert_eq!(
        parse("/connect muse"),
        Some(Command::Connect(Some(String::from("muse"))))
    );
    assert_eq!(
        parse("/connect openai x"),
        Some(Command::Connect(Some(String::from("openai"))))
    );
    assert!(valid_effort("default"));
    assert!(valid_effort("low"));
    assert!(valid_effort("medium"));
    assert!(valid_effort("high"));
    assert!(!valid_effort("ultra"));
    assert!(!valid_effort("HIGH"));
}

#[test]
fn parse_web_ok_et_aide() {
    assert_eq!(
        parse("/web rust async runtime"),
        Some(Command::Web(String::from("rust async runtime")))
    );
    assert_eq!(parse("/web"), Some(Command::Hint(WEB_USAGE)));
    assert_eq!(parse("/web   "), Some(Command::Hint(WEB_USAGE)));
    assert!(crate::texts::help_text().contains("/web <query>"));
}

#[test]
fn parse_tell_ok_et_rejets() {
    assert_eq!(
        parse("/tell 2 bonjour"),
        Some(Command::Tell {
            target: 2,
            text: String::from("bonjour")
        })
    );
    assert_eq!(parse("/tell"), Some(Command::Hint(TELL_USAGE)));
    assert_eq!(parse("/tell 2"), Some(Command::Hint(TELL_USAGE)));
    assert_eq!(parse("/tell x bonjour"), Some(Command::Hint(TELL_USAGE)));
    assert_eq!(parse("/tell 0 bonjour"), Some(Command::Hint(TELL_USAGE)));
}

#[test]
fn parse_ask_diese_optionnel_et_rejets() {
    let want = Some(Command::Ask {
        agent: 3,
        text: String::from("suite"),
    });
    assert_eq!(parse("/ask #3 suite"), want);
    assert_eq!(parse("/ask 3 suite"), want);
    assert_eq!(parse("/ask"), Some(Command::Hint(ASK_USAGE)));
    assert_eq!(parse("/ask #3"), Some(Command::Hint(ASK_USAGE)));
    assert_eq!(parse("/ask x suite"), Some(Command::Hint(ASK_USAGE)));
}

#[test]
fn parse_msg_cible_chat_agent_ou_diffusion() {
    assert_eq!(
        parse("/msg 2 bonjour"),
        Some(Command::Msg {
            to: MsgTarget::One(Target::Chat(2)),
            text: String::from("bonjour")
        })
    );
    assert_eq!(
        parse("/msg @2 bonjour"),
        Some(Command::Msg {
            to: MsgTarget::One(Target::Chat(2)),
            text: String::from("bonjour")
        })
    );
    assert_eq!(
        parse("/msg #3 suite"),
        Some(Command::Msg {
            to: MsgTarget::One(Target::Agent(3)),
            text: String::from("suite")
        })
    );
    assert_eq!(
        parse("/msg ALL go"),
        Some(Command::Msg {
            to: MsgTarget::All,
            text: String::from("go")
        })
    );
    assert_eq!(parse("/msg"), Some(Command::Hint(MSG_USAGE)));
    assert_eq!(parse("/msg 2"), Some(Command::Hint(MSG_USAGE)));
    assert_eq!(parse("/msg x bonjour"), Some(Command::Hint(MSG_USAGE)));
    assert_eq!(parse("/msg 0 bonjour"), Some(Command::Hint(MSG_USAGE)));
    assert_eq!(parse("/links"), Some(Command::Links));
}

#[test]
fn parse_link_unlink_deux_bouts_stricts() {
    let chat = LinkEnd::Chat(0);
    let agent = LinkEnd::Agent(3);
    assert_eq!(
        parse("/link 1 #3"),
        Some(Command::Link {
            from: chat,
            to: agent
        })
    );
    assert_eq!(
        parse("/unlink #3 @1"),
        Some(Command::Unlink {
            from: agent,
            to: chat
        })
    );
    assert_eq!(parse("/link 1"), Some(Command::Hint(LINK_USAGE)));
    assert_eq!(parse("/link 1 2 x"), Some(Command::Hint(LINK_USAGE)));
    assert_eq!(parse("/link x 2"), Some(Command::Hint(LINK_USAGE)));
    assert_eq!(parse("/unlink"), Some(Command::Hint(UNLINK_USAGE)));
    assert_eq!(parse("/unlink 1"), Some(Command::Hint(UNLINK_USAGE)));
}

#[test]
fn parse_agent_cible_optionnelle() {
    assert_eq!(
        parse("/agent @2 fais ça"),
        Some(Command::Agent {
            target: Some(2),
            prompt: String::from("fais ça")
        })
    );
    // `@` non suivi de chiffres = début du prompt.
    assert_eq!(
        parse("/agent @demain fais ça"),
        Some(Command::Agent {
            target: None,
            prompt: String::from("@demain fais ça")
        })
    );
    assert_eq!(
        parse("/agent @2"),
        Some(Command::Unknown(String::from("/agent")))
    );
    assert_eq!(parse("/agent @0 x"), Some(Command::Hint(AGENT_USAGE)));
}

#[test]
fn parse_settings_et_alias() {
    assert_eq!(parse("/settings"), Some(Command::Settings(String::new())));
    assert_eq!(
        parse("  /setting  "),
        Some(Command::Settings(String::new()))
    );
    assert_eq!(
        parse("/params openai"),
        Some(Command::Settings(String::from("openai")))
    );
    assert_eq!(
        parse("/settings add ds http://127.0.0.1:9/v1 m"),
        Some(Command::Settings(String::from(
            "add ds http://127.0.0.1:9/v1 m"
        )))
    );
}

#[test]
fn parse_spec_new_statut_run() {
    assert_eq!(
        parse("/spec new auth login flow"),
        Some(Command::SpecNew {
            name: String::from("auth"),
            idea: String::from("login flow")
        })
    );
    assert_eq!(
        parse("/spec auth"),
        Some(Command::SpecShow {
            name: String::from("auth")
        })
    );
    assert_eq!(
        parse("/spec auth run"),
        Some(Command::SpecRun {
            name: String::from("auth")
        })
    );
    // `newton` est un nom, pas la création.
    assert_eq!(
        parse("/spec newton"),
        Some(Command::SpecShow {
            name: String::from("newton")
        })
    );
    assert!(crate::texts::help_text().contains("/spec"));
}

#[test]
fn parse_spec_malforme_aide() {
    assert_eq!(parse("/spec"), Some(Command::Hint(SPEC_USAGE)));
    assert_eq!(parse("/spec new"), Some(Command::Hint(SPEC_USAGE)));
    assert_eq!(parse("/spec new auth"), Some(Command::Hint(SPEC_USAGE)));
    assert_eq!(parse("/spec auth run x"), Some(Command::Hint(SPEC_USAGE)));
    assert_eq!(parse("/spec auth x"), Some(Command::Hint(SPEC_USAGE)));
}

#[test]
fn parse_cloud_args_bruts() {
    assert_eq!(parse("/cloud"), Some(Command::Cloud(vec![])));
    assert_eq!(
        parse("/cloud run --repo o/r hello"),
        Some(Command::Cloud(vec![
            String::from("run"),
            String::from("--repo"),
            String::from("o/r"),
            String::from("hello"),
        ]))
    );
    assert!(crate::texts::help_text().contains("/cloud"));
}
