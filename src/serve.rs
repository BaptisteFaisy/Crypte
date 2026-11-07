//! Pilotage distant headless : HTTP + SSE.
//!
//! `run` expose l'`App` sans terminal : boucle 100 ms (`drain_replies` +
//! `tick`), `GET /` (client web), `GET /snapshot` (état JSON),
//! `GET /live` (flux SSE des [`Event`]), `POST /input` (envoi vers un chat).
//! Auth : `Authorization: Bearer <token>` partout ; `/live` accepte aussi
//! `?token=` (EventSource ne gère pas les en-têtes).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;

use crate::app::App;
use crate::chat::Role;
use crate::events::Event;
use crate::sessions::Sessions;

/// Client web servi sur `GET /` (authentifié comme le reste).
const WATCH_HTML: &str = include_str!("../assets/watch.html");
/// Événement SSE quand un abonné lent a raté des faits : le client
/// refait alors un `GET /snapshot` pour se recaler.
const LAGGED_DATA: &str = r#"{"type":"lagged"}"#;
/// Repli si un [`Event`] refusait la sérialisation (ne doit pas arriver).
const UNKNOWN_DATA: &str = r#"{"type":"unknown"}"#;
/// Rythme de la boucle headless (réponses + animations).
const TICK_MS: u64 = 100;
/// Battements SSE pour garder la connexion en vie.
const KEEP_ALIVE_SECS: u64 = 15;

/// État partagé : l'`App` sous verrou + le jeton d'authentification.
#[derive(Clone)]
struct Shared {
    app: Arc<Mutex<App>>,
    token: String,
}

/// Corps de `POST /input` : chat visé (1-based, comme la sidebar).
#[derive(Debug, Deserialize)]
struct InputBody {
    chat: usize,
    text: String,
}

/// Query de `GET /live` : `?token=` en secours du header bearer.
#[derive(Debug, Deserialize)]
struct LiveQuery {
    #[serde(default)]
    token: Option<String>,
}

/// Sert `app` en headless sur `addr` : boucle 100 ms + HTTP/SSE.
/// Aucun appel terminal ici (ni init ratatui, ni habillage Ghostty).
pub async fn run(app: App, addr: &str, token: String) -> Result<()> {
    let shared = Shared {
        app: Arc::new(Mutex::new(app)),
        token,
    };
    spawn_tick(Arc::clone(&shared.app));
    let router = build_router(shared);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("serve: cannot bind {addr}"))?;
    eprintln!("serve: pilotage distant sur http://{addr}");
    axum::serve(listener, router)
        .await
        .context("serve: server error")?;
    Ok(())
}

/// Boucle headless : toutes les 100 ms, verrouille puis route les
/// réponses arrivées et fait avancer les animations.
fn spawn_tick(app: Arc<Mutex<App>>) {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_millis(TICK_MS));
        loop {
            every.tick().await;
            let mut app = app.lock().await;
            app.drain_replies();
            app.tick();
        }
    });
}

/// Routes HTTP + état partagé.
fn build_router(shared: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/snapshot", get(snapshot))
        .route("/live", get(live))
        .route("/input", post(post_input))
        .with_state(shared)
}

/// `GET /` : le client web (HTML).
async fn index(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    Html(WATCH_HTML).into_response()
}

/// `GET /snapshot` : état complet des chats en JSON.
async fn snapshot(State(state): State<Shared>, headers: HeaderMap) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let app = state.app.lock().await;
    Json(build_snapshot(&app)).into_response()
}

/// `GET /live` : flux SSE des [`Event`] (JSON en `data`), battements
/// 15 s ; abonné lent → `data {"type":"lagged"}` (resnapshot client).
async fn live(
    State(state): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<LiveQuery>,
) -> Response {
    if !live_authorized(&headers, query.token.as_deref(), &state.token) {
        return unauthorized().into_response();
    }
    let rx = state.app.lock().await.events.subscribe();
    let stream = BroadcastStream::new(rx).map(|item| {
        let data = match item {
            Ok(event) => event_data(&event),
            Err(_) => String::from(LAGGED_DATA),
        };
        Ok::<_, axum::Error>(SseEvent::default().data(data))
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(KEEP_ALIVE_SECS)))
        .into_response()
}

/// `POST /input` : valide, sélectionne le chat, pose le texte puis
/// envoie (les `/commandes` sont routées par `send` comme au clavier).
async fn post_input(
    State(state): State<Shared>,
    headers: HeaderMap,
    body: Result<Json<InputBody>, JsonRejection>,
) -> Response {
    if !bearer_ok(&headers, &state.token) {
        return unauthorized().into_response();
    }
    let Json(body) = match body {
        Ok(body) => body,
        Err(_) => return bad_request("invalid JSON (want {\"chat\": N, \"text\": str})"),
    };
    let mut app = state.app.lock().await;
    let idx = match resolve_input(&app.sessions, body.chat, &body.text) {
        Ok(idx) => idx,
        Err(error) => return bad_request(error),
    };
    app.sessions.select(idx);
    app.sessions.current_mut().chat.input = body.text;
    app.send();
    Json(json!({"ok": true})).into_response()
}

/// Vrai si `Authorization: Bearer <token>` correspond (token vide refusé).
fn bearer_ok(headers: &HeaderMap, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let Some(value) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Some(given) = value.strip_prefix("Bearer ") else {
        return false;
    };
    given == token
}

/// Auth de `/live` : bearer OU `?token=` (secours EventSource).
fn live_authorized(headers: &HeaderMap, query_token: Option<&str>, token: &str) -> bool {
    if bearer_ok(headers, token) {
        return true;
    }
    !token.is_empty() && query_token == Some(token)
}

/// Valide un envoi : texte non vide + chat existant (1-based).
/// Rend l'index 0-based, ou le message d'erreur du 400.
fn resolve_input(sessions: &Sessions, chat: usize, text: &str) -> Result<usize, String> {
    if text.trim().is_empty() {
        return Err(String::from("empty text"));
    }
    let idx = chat.checked_sub(1).unwrap_or(usize::MAX);
    if sessions.get(idx).is_none() {
        return Err(format!("chat {chat} not found"));
    }
    Ok(idx)
}

/// État complet des chats (forme du `GET /snapshot`).
fn build_snapshot(app: &App) -> Value {
    let chats: Vec<Value> = app
        .sessions
        .iter()
        .map(|chat| {
            let messages: Vec<Value> = chat
                .chat
                .messages()
                .iter()
                .map(|m| json!({"role": role_name(m.role), "text": m.text}))
                .collect();
            let agents: Vec<Value> = chat
                .agents
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id,
                        "status": a.status.label(),
                        "prompt": a.prompt,
                        "prompt_tokens": a.prompt_tokens,
                        "completion_tokens": a.completion_tokens,
                    })
                })
                .collect();
            json!({
                "name": chat.name,
                "sending": chat.sending,
                "messages": messages,
                "prompt_tokens": chat.ledger.prompt,
                "completion_tokens": chat.ledger.completion,
                "agents": agents,
            })
        })
        .collect();
    json!({ "version": env!("CARGO_PKG_VERSION"), "chats": chats })
}

/// Nom JSON d'un rôle de message.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::System => "system",
    }
}

/// Un [`Event`] sérialisé pour le `data` SSE.
fn event_data(event: &Event) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| String::from(UNKNOWN_DATA))
}

/// 401 JSON : bearer manquant ou faux.
fn unauthorized() -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
}

/// 400 JSON : corps ou cible invalide.
fn bad_request(message: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": message.into()})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vrai bearer accepté, faux/absent/mal formé refusé.
    #[test]
    fn bearer_ok_exige_le_bon_token() {
        let headers = |value: Option<&str>| {
            let mut headers = HeaderMap::new();
            if let Some(value) = value {
                headers.insert(axum::http::header::AUTHORIZATION, value.parse().unwrap());
            }
            headers
        };
        assert!(bearer_ok(&headers(Some("Bearer secret")), "secret"));
        assert!(!bearer_ok(&headers(Some("Bearer faux")), "secret"));
        assert!(!bearer_ok(&headers(None), "secret"));
        assert!(!bearer_ok(&headers(Some("secret")), "secret"));
        assert!(!bearer_ok(&headers(Some("Bearer secret")), ""));
    }

    /// `/live` accepte le bearer comme le `?token=` (jamais sans rien).
    #[test]
    fn live_authorized_accepte_bearer_ou_query() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer secret".parse().unwrap(),
        );
        assert!(live_authorized(&headers, None, "secret"));
        assert!(live_authorized(&HeaderMap::new(), Some("secret"), "secret"));
        assert!(!live_authorized(&HeaderMap::new(), Some("faux"), "secret"));
        assert!(!live_authorized(&HeaderMap::new(), None, "secret"));
        assert!(!live_authorized(&HeaderMap::new(), Some("secret"), ""));
    }

    /// Envoi valide vers le chat 1 ; rejets : chat inconnu/0, texte vide.
    #[test]
    fn resolve_input_valide_et_rejette() {
        let sessions = Sessions::new();
        assert_eq!(resolve_input(&sessions, 1, "bonsoir"), Ok(0));
        assert_eq!(
            resolve_input(&sessions, 2, "bonsoir"),
            Err(String::from("chat 2 not found"))
        );
        assert_eq!(
            resolve_input(&sessions, 0, "bonsoir"),
            Err(String::from("chat 0 not found"))
        );
        assert_eq!(
            resolve_input(&sessions, 1, "   "),
            Err(String::from("empty text"))
        );
    }

    /// Snapshot hors ligne : forme exacte (chats, messages, agents, tokens).
    #[test]
    fn snapshot_depuis_offline_a_la_bonne_forme() {
        let mut app = App::offline().unwrap();
        let current = app.sessions.current_mut();
        current.chat.push(Role::User, "bonsoir la crypte");
        current.chat.push(Role::Assistant, "bienvenue, mortel");
        current.ledger.record(10, 5, 1.0);
        app.sessions.add_agent(
            0,
            "creuse un tunnel",
            vec![(String::from("user"), String::from("u"))],
        );
        let snap = build_snapshot(&app);
        assert!(!snap["version"].as_str().unwrap_or("").is_empty());
        assert_eq!(snap["chats"][0]["name"], "Chat 1");
        assert_eq!(snap["chats"][0]["sending"], false);
        assert_eq!(snap["chats"][0]["messages"][0]["role"], "user");
        assert_eq!(snap["chats"][0]["messages"][0]["text"], "bonsoir la crypte");
        assert_eq!(snap["chats"][0]["messages"][1]["role"], "assistant");
        assert_eq!(snap["chats"][0]["prompt_tokens"], 10);
        assert_eq!(snap["chats"][0]["completion_tokens"], 5);
        assert_eq!(snap["chats"][0]["agents"][0]["id"], 1);
        assert_eq!(snap["chats"][0]["agents"][0]["status"], "running");
        assert_eq!(snap["chats"][0]["agents"][0]["prompt"], "creuse un tunnel");
    }
}
