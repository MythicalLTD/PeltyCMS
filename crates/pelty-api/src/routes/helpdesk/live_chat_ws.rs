//! WebSockets accept secrets only in the first frame, never a URL or protocol.
//! Durable writes remain REST operations with the existing CSRF protections.
use crate::{
    chat_bus::{self, Event, Signal},
    error::ApiResult,
    extract::{self, perms, CurrentUser, RequirePermission},
    AppState,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::HeaderMap,
    response::Response,
    routing::get,
    Router,
};
use pelty_core::AppError;
use pelty_database::live_chat as db;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat/sessions/{id}/ws", get(visitor))
        .route("/admin/chats/{id}/ws", get(staff))
        .route("/admin/chats/ws", get(inbox))
}
/// Loopback host aliases browsers treat as different origins (`localhost` vs `127.0.0.1`).
fn loopback_alias(host: &str) -> Option<&'static str> {
    match host {
        "localhost" | "127.0.0.1" => Some("127.0.0.1"),
        "::1" | "[::1]" => Some("::1"),
        _ => None,
    }
}

fn same_site_origin(origin: &str, public_url: &str) -> bool {
    let Ok(want) = reqwest::Url::parse(public_url) else {
        return false;
    };
    let Ok(got) = reqwest::Url::parse(origin) else {
        return false;
    };
    if got.origin().ascii_serialization() == want.origin().ascii_serialization() {
        return true;
    }
    // cargo dep: UI on :5173, API on :8080; both must share a loopback host.
    let (Some(gh), Some(wh)) = (got.host_str(), want.host_str()) else {
        return false;
    };
    let Some(g) = loopback_alias(gh) else {
        return false;
    };
    let Some(w) = loopback_alias(wh) else {
        return false;
    };
    if g != w || got.scheme() != want.scheme() {
        return false;
    }
    let gp = got.port_or_known_default();
    let wp = want.port_or_known_default();
    gp == wp
        || matches!(
            (gp, wp),
            (Some(5173), Some(8080)) | (Some(8080), Some(5173))
        )
}

fn origin(headers: &HeaderMap, public_url: &str) -> bool {
    let mut values = headers.get_all("origin").iter();
    let Some(value) = values.next().and_then(|v| v.to_str().ok()) else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    same_site_origin(value, public_url)
}
fn guard(s: &AppState, h: &HeaderMap, key: &str) -> ApiResult<()> {
    if !origin(h, &s.cfg.public_url) {
        return Err(AppError::Forbidden.into());
    }
    if !s.auth_limiter.check(key) {
        return Err(AppError::RateLimited.into());
    }
    Ok(())
}
#[utoipa::path(get, path = "/api/v1/chat/sessions/{id}/ws", responses((status = 101, description = "First text frame: {token, after}. Server: sync, signal. Client: typing."), (status = 403)))]
pub async fn visitor(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    ext: axum::http::Extensions,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    guard(&s, &h, &format!("chat:ws:{}", super::ip_of(&ext)))?;
    super::enabled(&s).await?;
    let permit = s
        .chat_bus
        .connections
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::RateLimited)?;
    Ok(ws
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| run(s, socket, id, None, permit)))
}
async fn staff_upgrade(
    s: AppState,
    id: Uuid,
    h: HeaderMap,
    u: CurrentUser,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    guard(&s, &h, &format!("chat:ws:staff:{}", u.user_id))?;
    // Do not let GET's tickets.read fallback grant a writable staff channel.
    if u.api_key || !u.has("tickets.manage") {
        return Err(AppError::Forbidden.into());
    }
    if !id.is_nil() {
        db::status(&s.pool, id).await?;
    }
    let raw = extract::cookie(&h, extract::SESSION_COOKIE).ok_or(AppError::Unauthenticated)?;
    let permit = s
        .chat_bus
        .connections
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::RateLimited)?;
    Ok(ws
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| run(s, socket, id, Some((u, raw)), permit)))
}
#[utoipa::path(get, path = "/api/v1/admin/chats/{id}/ws", responses((status = 101, description = "Session cookie and first frame {csrf, after} required"), (status = 403)))]
pub async fn staff(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    staff_upgrade(s, id, h, u, ws).await
}
#[utoipa::path(get, path = "/api/v1/admin/chats/ws", responses((status = 101, description = "Staff inbox refresh stream; first frame {csrf}"), (status = 403)))]
pub async fn inbox(
    State(s): State<AppState>,
    h: HeaderMap,
    RequirePermission(u, _): RequirePermission<perms::TicketsManage>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    staff_upgrade(s, Uuid::nil(), h, u, ws).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    token: Option<String>,
    csrf: Option<String>,
    #[serde(default)]
    after: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Typing {
    typing: bool,
}
async fn run(
    s: AppState,
    mut socket: WebSocket,
    id: Uuid,
    staff: Option<(CurrentUser, String)>,
    _permit: tokio::sync::OwnedSemaphorePermit,
) {
    let Ok(Some(Ok(Message::Text(first)))) =
        tokio::time::timeout(Duration::from_secs(5), socket.recv()).await
    else {
        return;
    };
    let Ok(hello) = serde_json::from_str::<Hello>(&first) else {
        return;
    };
    if hello.after < 0 {
        return;
    }
    if let Some((u, _)) = &staff {
        if !hello
            .csrf
            .as_deref()
            .is_some_and(|v| pelty_auth::token::constant_eq(v, &u.csrf_token))
        {
            return;
        }
    } else if !hello
        .token
        .as_ref()
        .is_some_and(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return;
    }
    if !authorized(&s, id, &staff, hello.token.as_deref()).await {
        return;
    }
    chat_bus::start(&s);
    let connection = Uuid::new_v4();
    let mut signals = s.chat_bus.tx.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut after = hello.after;
    let mut last_input = std::time::Instant::now() - Duration::from_secs(1);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3600);
    let mut signal = Signal {
        chat_id: id,
        connection,
        staff: staff.is_some(),
        typing: false,
        online: true,
    };
    let mut last_seen = std::time::Instant::now();
    loop {
        if last_seen.elapsed() > Duration::from_secs(45) {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            _ = tick.tick() => {
                let Some(payload) = sync_payload(&s, id, &staff, hello.token.as_deref(), &mut after).await else { break };
                if !send(&mut socket, payload).await { break; }
                if !matches!(tokio::time::timeout(Duration::from_secs(3), socket.send(Message::Ping(Vec::new().into()))).await, Ok(Ok(()))) { break; }
                if !id.is_nil() { if last_input.elapsed() > Duration::from_secs(3) { signal.typing = false; } chat_bus::publish(&s, signal.clone()).await; }
            }
            message = socket.recv() => {
                last_seen = std::time::Instant::now();
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if last_input.elapsed() < Duration::from_millis(500) { continue; }
                        last_input = std::time::Instant::now();
                        let Ok(input) = serde_json::from_str::<Typing>(&text) else { break };
                        if id.is_nil() { break; }
                        signal.typing = input.typing;
                        chat_bus::publish(&s, signal.clone()).await;
                    }
                    Some(Ok(Message::Ping(v))) => { if !matches!(tokio::time::timeout(Duration::from_secs(3), socket.send(Message::Pong(v))).await, Ok(Ok(()))) { break; } }
                    Some(Ok(Message::Pong(_))) => {},
                    _ => break,
                }
            }
            event = signals.recv() => {
                match event {
                    Ok(event) if event.invalidates(id) => {
                        let Some(payload) = sync_payload(&s, id, &staff, hello.token.as_deref(), &mut after).await else { break };
                        if !send(&mut socket, payload).await { break; }
                    }
                    Ok(Event::Presence(event)) if event.chat_id == id && event.connection != connection && event.staff != staff.is_some() => {
                        if !authorized(&s, id, &staff, hello.token.as_deref()).await { break; }
                        if !send(&mut socket, json!({"type":"signal", "connection":event.connection,"staff":event.staff,"online":event.online,"typing":event.typing})).await { break; }
                    }
                    _ => {}, // Lost hints recover on the two-second tick.
                }
            }
        }
    }
    signal.online = false;
    signal.typing = false;
    if !id.is_nil() {
        chat_bus::publish(&s, signal).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Close(None))).await;
}
async fn authorized(
    s: &AppState,
    id: Uuid,
    staff: &Option<(CurrentUser, String)>,
    token: Option<&str>,
) -> bool {
    if let Some((u, raw)) = staff {
        pelty_database::sessions::resolve(
            &s.pool,
            &pelty_auth::token::hash(raw),
            s.auth.idle,
            time::Duration::seconds(60),
        )
        .await
        .ok()
        .flatten()
        .is_some_and(|ctx| {
            ctx.user_id == u.user_id && ctx.permissions.iter().any(|p| p == "tickets.manage")
        })
    } else {
        db::authorize(&s.pool, id, token.unwrap_or(""))
            .await
            .is_ok()
            && super::enabled(s).await.is_ok()
    }
}
/// Every delivery path revalidates access, including hints arriving between recovery ticks.
async fn sync_payload(
    s: &AppState,
    id: Uuid,
    staff: &Option<(CurrentUser, String)>,
    token: Option<&str>,
    after: &mut i64,
) -> Option<serde_json::Value> {
    if !authorized(s, id, staff, token).await {
        return None;
    }
    if id.is_nil() {
        return Some(json!({"type":"inbox"}));
    }
    let status = db::status(&s.pool, id).await.ok()?;
    let messages = db::messages(
        &s.pool,
        id,
        *after,
        if staff.is_some() { "staff" } else { "visitor" },
    )
    .await
    .ok()?;
    if let Some(m) = messages.last() {
        *after = m.id;
    }
    Some(json!({"type":"sync", "status":status, "messages":messages}))
}
async fn send(socket: &mut WebSocket, value: serde_json::Value) -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(3),
            socket.send(Message::Text(value.to_string().into()))
        )
        .await,
        Ok(Ok(()))
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_is_exact_and_required() {
        let mut h = HeaderMap::new();
        assert!(!origin(&h, "https://panel.example"));
        h.insert("origin", "https://panel.example".parse().unwrap());
        assert!(origin(&h, "https://panel.example/base"));
        assert!(!origin(&h, "http://panel.example"));
        h.insert("origin", "https://panel.example.evil".parse().unwrap());
        assert!(!origin(&h, "https://panel.example"));
        h.insert("origin", "null".parse().unwrap());
        assert!(!origin(&h, "https://panel.example"));
    }
    #[test]
    fn origin_allows_local_vite_ui_against_api_public_url() {
        let mut h = HeaderMap::new();
        h.insert("origin", "http://127.0.0.1:5173".parse().unwrap());
        assert!(origin(&h, "http://localhost:8080"));
        h.insert("origin", "http://localhost:5173".parse().unwrap());
        assert!(origin(&h, "http://127.0.0.1:8080"));
        h.insert("origin", "http://127.0.0.1:5173".parse().unwrap());
        assert!(!origin(&h, "https://panel.example"));
    }
}
