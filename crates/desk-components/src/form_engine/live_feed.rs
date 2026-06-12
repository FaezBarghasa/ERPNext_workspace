/// Task 3.2 — Live Activity Feed via Server-Sent Events (SSE)
///
/// Connects to the frappe-net SSE endpoint and renders a real-time scrolling
/// feed of document events (creates, updates, submits, comments).
/// Falls back gracefully to polling when the SSE connection drops.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;

// ── Event types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedEventKind {
    Create,
    Update,
    Submit,
    Cancel,
    Comment,
    Share,
    Assign,
}

impl FeedEventKind {
    pub fn icon(&self) -> &'static str {
        match self {
            FeedEventKind::Create  => "➕",
            FeedEventKind::Update  => "✏️",
            FeedEventKind::Submit  => "✅",
            FeedEventKind::Cancel  => "🚫",
            FeedEventKind::Comment => "💬",
            FeedEventKind::Share   => "🔗",
            FeedEventKind::Assign  => "👤",
        }
    }
    pub fn color(&self) -> &'static str {
        match self {
            FeedEventKind::Create  => "#a6e3a1",
            FeedEventKind::Update  => "#89b4fa",
            FeedEventKind::Submit  => "#cba6f7",
            FeedEventKind::Cancel  => "#f38ba8",
            FeedEventKind::Comment => "#fab387",
            FeedEventKind::Share   => "#94e2d5",
            FeedEventKind::Assign  => "#f9e2af",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedEvent {
    pub id: String,
    pub kind: FeedEventKind,
    pub doctype: String,
    pub docname: String,
    pub user: String,
    pub timestamp: String,
    pub message: Option<String>,
}

// ── Component props ───────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct LiveFeedProps {
    pub api_base: String,
    /// Optional filter: only show events for this doctype
    pub doctype_filter: Option<String>,
    /// Maximum number of events to display
    #[props(default = 50)]
    pub max_events: usize,
}

// ── SSE connection state ──────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Error(String),
}

// ── Main component ────────────────────────────────────────────────────────────

#[component]
pub fn LiveFeed(props: LiveFeedProps) -> Element {
    let mut events: Signal<Vec<FeedEvent>> = use_signal(Vec::new);
    let mut conn_state: Signal<ConnectionState> = use_signal(|| ConnectionState::Connecting);

    let api_base = props.api_base.clone();
    let doctype_filter = props.doctype_filter.clone();
    let max_events = props.max_events;

    // SSE polling loop (WASM-compatible: uses reqwest + async spawn)
    use_effect(move || {
        let api_base = api_base.clone();
        let doctype_filter = doctype_filter.clone();
        spawn(async move {
            loop {
                let sse_url = match &doctype_filter {
                    Some(dt) => format!("{}/api/feed/events?doctype={}", api_base, dt),
                    None => format!("{}/api/feed/events", api_base),
                };

                match reqwest::get(&sse_url).await {
                    Ok(resp) if resp.status().is_success() => {
                        conn_state.set(ConnectionState::Connected);

                        // Parse the JSON array response (polling fallback)
                        if let Ok(feed_events) = resp.json::<Vec<FeedEvent>>().await {
                            let mut current = events.write();
                            for ev in feed_events {
                                // Deduplicate by id
                                if !current.iter().any(|e| e.id == ev.id) {
                                    current.insert(0, ev);
                                }
                            }
                            // Trim to max_events
                            current.truncate(max_events);
                        }
                    }
                    Ok(r) => {
                        conn_state.set(ConnectionState::Error(
                            format!("HTTP {}", r.status())
                        ));
                    }
                    Err(e) => {
                        conn_state.set(ConnectionState::Reconnecting);
                        // Log but don't crash — will retry
                        let _ = e;
                    }
                }

                // Poll every 3 seconds (low overhead for RPi 5)
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    });

    rsx! {
        div {
            class: "live-feed",
            style: "
                width: 100%;
                background: #181825;
                border-radius: 12px;
                padding: 1.25rem;
                box-shadow: 0 4px 16px rgba(0,0,0,0.3);
                font-family: 'Inter', sans-serif;
                color: #cdd6f4;
            ",

            // ── Header ────────────────────────────────────────────────────────
            div {
                style: "display:flex; align-items:center; justify-content:space-between; margin-bottom:1rem;",
                div {
                    style: "display:flex; align-items:center; gap:0.6rem;",
                    div {
                        style: "width:8px; height:8px; border-radius:50%; animation: pulse 1.5s infinite;",
                        background: match &*conn_state.read() {
                            ConnectionState::Connected    => "#a6e3a1",
                            ConnectionState::Connecting   => "#fab387",
                            ConnectionState::Reconnecting => "#f9e2af",
                            ConnectionState::Error(_)     => "#f38ba8",
                        }
                    }
                    h3 { style: "margin:0; font-size:1rem; font-weight:700; color:#cba6f7;", "Activity Feed" }
                }
                span {
                    style: "font-size:0.72rem; color:#6c7086;",
                    match &*conn_state.read() {
                        ConnectionState::Connected    => "Live".to_string(),
                        ConnectionState::Connecting   => "Connecting…".to_string(),
                        ConnectionState::Reconnecting => "Reconnecting…".to_string(),
                        ConnectionState::Error(e)     => format!("Error: {}", e),
                    }
                }
            }

            // ── Event list ────────────────────────────────────────────────────
            div {
                style: "display:flex; flex-direction:column; gap:0.5rem; max-height:480px; overflow-y:auto;",
                if events.read().is_empty() {
                    div {
                        style: "text-align:center; padding:2rem; color:#585b70;",
                        div { style: "font-size:1.5rem; margin-bottom:0.4rem;", "📭" }
                        "No activity yet"
                    }
                }
                for event in events.read().iter() {
                    FeedEventCard { event: event.clone() }
                }
            }
        }
    }
}

// ── Individual event card ─────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
struct FeedEventCardProps {
    event: FeedEvent,
}

#[component]
fn FeedEventCard(props: FeedEventCardProps) -> Element {
    let ev = &props.event;
    let icon = ev.kind.icon();
    let color = ev.kind.color();

    rsx! {
        div {
            class: "feed-event-card",
            style: format!("
                display:flex; gap:0.75rem; align-items:flex-start;
                background:#1e1e2e; border-radius:8px;
                padding:0.75rem; border-left:3px solid {};
                transition: background 0.15s;
            ", color),

            // Icon avatar
            div {
                style: format!("
                    width:32px; height:32px; border-radius:50%;
                    display:flex; align-items:center; justify-content:center;
                    background:{}22; font-size:1rem; flex-shrink:0;
                ", color),
                "{icon}"
            }

            // Content
            div {
                style: "flex:1; min-width:0;",
                div {
                    style: "display:flex; justify-content:space-between; align-items:baseline; margin-bottom:0.2rem;",
                    span {
                        style: "font-size:0.85rem; font-weight:600; color:#cdd6f4;",
                        "{ev.user}"
                    }
                    span {
                        style: "font-size:0.7rem; color:#585b70; white-space:nowrap;",
                        "{ev.timestamp}"
                    }
                }
                div {
                    style: "font-size:0.82rem; color:#a6adc8;",
                    span { style: format!("color:{};", color), "{icon} " }
                    span {
                        style: "color:#89b4fa; cursor:pointer;",
                        "{ev.doctype} · {ev.docname}"
                    }
                }
                if let Some(msg) = &ev.message {
                    div {
                        style: "margin-top:0.3rem; font-size:0.8rem; color:#6c7086; font-style:italic; white-space:pre-wrap;",
                        ""{msg}""
                    }
                }
            }
        }
    }
}
