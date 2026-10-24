use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// A single event received from the Qt WebSocket API.
/// Events are messages without an "id" field, pushed by Qt's broadcastEvent().
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Event type name, e.g. "connectionStateChanged", "clipboardChanged"
    pub event: String,
    /// Event payload
    pub data: Value,
    /// Timestamp in milliseconds since UNIX epoch
    pub timestamp: u64,
}

/// Event bus that caches recent events and provides pub/sub functionality.
///
/// - Qt side sends `{ "event": "...", "data": {...} }` via WebSocket
/// - `ws_client.rs` receives these and calls `publish()`
/// - MCP tools can call `wait_for()` to block until a matching event arrives
/// - Recent events are kept in a ring buffer for `recent_events()` queries
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

struct EventBusInner {
    /// Broadcast sender for real-time event dispatch
    sender: broadcast::Sender<Event>,
    /// Ring buffer of recent events
    history: RwLock<VecDeque<Event>>,
    /// Maximum number of events to keep in history
    max_history: usize,
}

impl EventBus {
    pub fn new(max_history: usize) -> Self {
        let (sender, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(EventBusInner {
                sender,
                history: RwLock::new(VecDeque::with_capacity(max_history)),
                max_history,
            }),
        }
    }

    /// Push a new event into the bus.
    /// Stores in history ring buffer and broadcasts to all subscribers.
    pub async fn publish(&self, event: Event) {
        {
            let mut history = self.inner.history.write().await;
            if history.len() >= self.inner.max_history {
                history.pop_front();
            }
            history.push_back(event.clone());
        }
        // Broadcast to subscribers (ignore error if no receivers)
        let _ = self.inner.sender.send(event);
    }

    /// Subscribe to receive new events via a broadcast receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.sender.subscribe()
    }

    /// Get recent events, optionally filtered by event type.
    /// Returns events in chronological order (oldest first).
    pub async fn recent_events(&self, event_type: Option<&str>, limit: usize) -> Vec<Event> {
        let history = self.inner.history.read().await;
        history
            .iter()
            .rev()
            .filter(|e| match event_type {
                Some(t) => e.event == t,
                None => true,
            })
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Wait for an event matching criteria with timeout.
    ///
    /// When `check_history` is true, the history buffer is checked first so that
    /// events that fired between the triggering action and this call are not
    /// missed (avoids race conditions).  Set to false for tools like
    /// `wait_for_clipboard_change` where only **new** events are meaningful.
    pub async fn wait_for(
        &self,
        event_type: &str,
        filter: Option<&Value>,
        timeout_ms: u64,
        check_history: bool,
    ) -> Result<Event, String> {
        // Subscribe BEFORE checking history to avoid missing events in the gap
        let mut rx = self.subscribe();

        if check_history {
            let history = self.inner.history.read().await;
            // Search from newest to oldest, return the most recent match
            for event in history.iter().rev() {
                if event.event == event_type {
                    let matched = match filter {
                        Some(f) => matches_filter(&event.data, f),
                        None => true,
                    };
                    if matched {
                        return Ok(event.clone());
                    }
                }
            }
        }

        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!(
                    "Timeout waiting for event '{}' after {}ms",
                    event_type, timeout_ms
                ));
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(event)) => {
                    if event.event == event_type {
                        if let Some(filter_val) = filter {
                            if matches_filter(&event.data, filter_val) {
                                return Ok(event);
                            }
                        } else {
                            return Ok(event);
                        }
                    }
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    // Missed some events due to slow consumer, continue waiting
                    continue;
                }
                Ok(Err(_)) => {
                    return Err("Event bus closed".to_string());
                }
                Err(_) => {
                    return Err(format!(
                        "Timeout waiting for event '{}' after {}ms",
                        event_type, timeout_ms
                    ));
                }
            }
        }
    }
}

/// Check if event data matches a filter object.
/// Each key-value pair in the filter must be present and equal in data.
fn matches_filter(data: &Value, filter: &Value) -> bool {
    if let (Some(data_obj), Some(filter_obj)) = (data.as_object(), filter.as_object()) {
        for (key, expected) in filter_obj {
            match data_obj.get(key) {
                Some(actual) if actual == expected => continue,
                _ => return false,
            }
        }
        true
    } else {
        data == filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_publish_and_recent() {
        let bus = EventBus::new(100);
        bus.publish(Event {
            event: "test".to_string(),
            data: json!({"key": "value"}),
            timestamp: 1000,
        })
        .await;

        let events = bus.recent_events(None, 10).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "test");
    }

    #[tokio::test]
    async fn test_filter_by_type() {
        let bus = EventBus::new(100);
        bus.publish(Event {
            event: "a".to_string(),
            data: json!({}),
            timestamp: 1,
        })
        .await;
        bus.publish(Event {
            event: "b".to_string(),
            data: json!({}),
            timestamp: 2,
        })
        .await;

        let events = bus.recent_events(Some("a"), 10).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "a");
    }

    #[tokio::test]
    async fn test_ring_buffer_overflow() {
        let bus = EventBus::new(3);
        for i in 0..5 {
            bus.publish(Event {
                event: format!("e{i}"),
                data: json!({}),
                timestamp: i as u64,
            })
            .await;
        }

        let events = bus.recent_events(None, 10).await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event, "e2");
        assert_eq!(events[2].event, "e4");
    }

    #[tokio::test]
    async fn test_matches_filter() {
        assert!(matches_filter(
            &json!({"state": "connected", "id": "conn_1"}),
            &json!({"state": "connected"})
        ));
        assert!(!matches_filter(
            &json!({"state": "disconnected"}),
            &json!({"state": "connected"})
        ));
    }

    #[tokio::test]
    async fn test_wait_for_timeout() {
        let bus = EventBus::new(100);
        let result = bus.wait_for("nonexistent", None, 50, true).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Timeout"));
    }

    #[tokio::test]
    async fn test_wait_for_success() {
        let bus = EventBus::new(100);
        let bus2 = bus.clone();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            bus2.publish(Event {
                event: "connectionStateChanged".to_string(),
                data: json!({"deviceId": "conn_1", "state": "connected"}),
                timestamp: 100,
            })
            .await;
        });

        let result = bus
            .wait_for(
                "connectionStateChanged",
                Some(&json!({"state": "connected"})),
                2000,
                true,
            )
            .await;
        assert!(result.is_ok());
        let event = result.unwrap();
        assert_eq!(event.event, "connectionStateChanged");
    }
}
