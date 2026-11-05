//! Presence and durable-message invalidation hints. Transcript content stays in Postgres.
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::sync::broadcast;
use uuid::Uuid;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signal {
    pub chat_id: Uuid,
    pub connection: Uuid,
    pub staff: bool,
    pub typing: bool,
    pub online: bool,
}
/// Explicit kinds keep ephemeral typing/presence separate from transcript changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Presence(Signal),
    MessagesChanged { chat_id: Uuid },
}
impl Event {
    pub fn invalidates(&self, chat_id: Uuid) -> bool {
        matches!(self, Self::MessagesChanged { chat_id: changed } if chat_id.is_nil() || *changed == chat_id)
    }
}
pub struct ChatBus {
    pub tx: broadcast::Sender<Event>,
    pub connections: std::sync::Arc<tokio::sync::Semaphore>,
    started: AtomicBool,
}
impl Default for ChatBus {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(256).0,
            connections: std::sync::Arc::new(tokio::sync::Semaphore::new(128)),
            started: AtomicBool::new(false),
        }
    }
}
fn channel(s: &AppState) -> String {
    format!("{}:chat", crate::cluster::channel(s))
}
pub fn start(s: &AppState) {
    if !s.shared.enabled() || s.chat_bus.started.swap(true, Ordering::AcqRel) {
        return;
    }
    let s = s.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = listen(&s).await {
                tracing::warn!(error = %e, "chat subscription lost; durable sync remains available");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}
async fn listen(s: &AppState) -> Result<(), String> {
    use futures_util::StreamExt;
    let client =
        redis::Client::open(s.shared.url().unwrap_or_default()).map_err(|e| e.to_string())?;
    let mut ps = tokio::time::timeout(Duration::from_secs(5), client.get_async_pubsub())
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| e.to_string())?;
    ps.subscribe(channel(s)).await.map_err(|e| e.to_string())?;
    let mut stream = ps.into_on_message();
    while let Some(msg) = stream.next().await {
        let Ok(body) = msg.get_payload::<String>() else {
            continue;
        };
        if body.len() > 2048 {
            continue;
        }
        let Some((instance, signal)) = decode(&body) else {
            continue;
        };
        if instance != s.shared.instance {
            let _ = s.chat_bus.tx.send(signal);
        }
    }
    Err("stream ended".into())
}
fn decode(body: &str) -> Option<(String, Event)> {
    let (instance, value): (String, serde_json::Value) = serde_json::from_str(body).ok()?;
    // Accept the original presence wire format during rolling upgrades; a
    // malformed/unknown explicit kind must never fall back to a message hint.
    let event = if value.get("kind").is_some() {
        serde_json::from_value(value).ok()?
    } else {
        Event::Presence(serde_json::from_value(value).ok()?)
    };
    Some((instance, event))
}
pub async fn publish(s: &AppState, signal: Signal) {
    publish_event(s, Event::Presence(signal)).await;
}
/// Call only after the message transaction commits; this carries no message or credentials.
pub async fn messages_changed(s: &AppState, chat_id: Uuid) {
    publish_event(s, Event::MessagesChanged { chat_id }).await;
}
async fn publish_event(s: &AppState, signal: Event) {
    let _ = s.chat_bus.tx.send(signal.clone());
    if !s.shared.enabled() {
        return;
    }
    let work = async {
        let Some(mut c) = s.shared.conn().await else {
            return;
        };
        let msg = serde_json::json!([s.shared.instance, signal]).to_string();
        let result = redis::cmd("PUBLISH")
            .arg(channel(s))
            .arg(msg)
            .query_async::<i64>(&mut c)
            .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, "chat event publish failed");
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), work).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hints_are_scoped_and_contain_no_transcript_content() {
        let chat_id = Uuid::new_v4();
        let event = Event::MessagesChanged { chat_id };
        assert!(event.invalidates(chat_id));
        assert!(
            event.invalidates(Uuid::nil()),
            "staff inbox refreshes for any chat"
        );
        assert!(!event.invalidates(Uuid::new_v4()));
        let value = serde_json::json!(["instance", event]);
        assert_eq!(
            value[1],
            serde_json::json!({"kind":"messages_changed","chat_id":chat_id})
        );
        assert!(decode(&value.to_string()).unwrap().1.invalidates(chat_id));
    }
    #[test]
    fn typing_and_legacy_presence_never_invalidate_messages() {
        let signal = Signal {
            chat_id: Uuid::new_v4(),
            connection: Uuid::new_v4(),
            staff: false,
            typing: true,
            online: true,
        };
        for value in [
            serde_json::json!(["instance", signal.clone()]),
            serde_json::json!(["instance", Event::Presence(signal.clone())]),
        ] {
            let (_, event) = decode(&value.to_string()).unwrap();
            assert!(matches!(event, Event::Presence(_)));
            assert!(!event.invalidates(signal.chat_id));
            assert!(!event.invalidates(Uuid::nil()));
        }
        assert!(decode(
            &serde_json::json!(["instance", {"kind":"typing", "chat_id":signal.chat_id}])
                .to_string()
        )
        .is_none());
    }
}
