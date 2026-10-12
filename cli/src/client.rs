use crate::app::{BookSnapshot, EventEntry, EventKind, MonitorState};
use chrono::Local;
use futures_util::StreamExt;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::connect_async;

#[derive(Deserialize)]
struct ApiBookResponse {
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

fn sym_str(v: &serde_json::Value) -> String {
    // Symbol is {"0": "AAPL"} or just "AAPL"
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(obj) = v.as_object() {
        if let Some(inner) = obj.get("0").and_then(|x| x.as_str()) {
            return inner.to_string();
        }
        // Sometimes serialised as {"inner": "AAPL"}
        for val in obj.values() {
            if let Some(s) = val.as_str() {
                return s.to_string();
            }
        }
    }
    v.to_string()
}

pub async fn poll_books(state: Arc<Mutex<MonitorState>>) {
    let client = reqwest::Client::new();
    loop {
        let (base_url, symbols) = {
            let s = state.lock().unwrap();
            (s.base_url.clone(), s.symbols.clone())
        };

        for sym in &symbols {
            let url = format!("{}/v1/book/{}", base_url, sym);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<ApiBookResponse>().await {
                        let bids = parse_levels(&body.bids);
                        let asks = parse_levels(&body.asks);
                        let mut s = state.lock().unwrap();
                        s.books.insert(
                            sym.clone(),
                            BookSnapshot {
                                bids,
                                asks,
                                updated_at: std::time::Instant::now(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn parse_levels(levels: &[[String; 2]]) -> Vec<(Decimal, Decimal)> {
    levels
        .iter()
        .filter_map(|[p, q]| {
            let price = p.parse::<Decimal>().ok()?;
            let qty = q.parse::<Decimal>().ok()?;
            Some((price, qty))
        })
        .collect()
}

#[derive(Deserialize)]
struct SessionResponse {
    state: serde_json::Value,
}

pub async fn poll_sessions(state: Arc<Mutex<MonitorState>>) {
    let client = reqwest::Client::new();
    loop {
        let (base_url, symbols) = {
            let s = state.lock().unwrap();
            (s.base_url.clone(), s.symbols.clone())
        };

        for sym in &symbols {
            let url = format!("{}/v1/session/{}", base_url, sym);
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<SessionResponse>().await {
                        let state_str = state_value_to_string(&body.state);
                        let mut s = state.lock().unwrap();
                        s.sessions.insert(sym.clone(), state_str);
                        s.connected = true;
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    let mut s = state.lock().unwrap();
                    s.connected = false;
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn state_value_to_string(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(obj) = v.as_object() {
        // e.g. {"Halted": {"reason": "CircuitBreaker", ...}}
        if let Some(key) = obj.keys().next() {
            return key.clone();
        }
    }
    v.to_string()
}

pub async fn subscribe_ws(state: Arc<Mutex<MonitorState>>) {
    loop {
        let base_url = state.lock().unwrap().base_url.clone();
        let ws_url = base_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1);
        let full_url = format!("{}/v1/stream", ws_url);

        match connect_async(&full_url).await {
            Ok((ws_stream, _)) => {
                state.lock().unwrap().connected = true;
                let (_, mut read) = ws_stream.split();
                while let Some(msg) = read.next().await {
                    match msg {
                        Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                            handle_ws_message(&text, &state);
                        }
                        Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
                        Err(_) => break,
                        _ => {}
                    }
                }
                state.lock().unwrap().connected = false;
            }
            Err(_) => {
                state.lock().unwrap().connected = false;
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn handle_ws_message(text: &str, state: &Arc<Mutex<MonitorState>>) {
    let val: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };

    let event_tag = val.get("event").and_then(|v| v.as_str()).unwrap_or("");

    let mut s = state.lock().unwrap();
    match event_tag {
        "trade" => {
            let symbol = val
                .get("symbol")
                .map(sym_str)
                .unwrap_or_default();
            let price = val.get("price").and_then(|v| v.as_str()).unwrap_or("?");
            let qty = val.get("quantity").and_then(|v| v.as_str()).unwrap_or("?");
            s.push_event(EventEntry {
                ts: Local::now(),
                kind: EventKind::Trade,
                symbol: symbol.clone(),
                detail: format!("{}  x {}  price={}", symbol, qty, price),
            });
            s.metrics.record_trade();
        }
        "order_accepted" => {
            let symbol = val
                .get("symbol")
                .map(sym_str)
                .unwrap_or_default();
            let id = val
                .get("order_id")
                .and_then(|v| v.get("0"))
                .and_then(|v| v.as_str())
                .or_else(|| val.get("order_id").and_then(|v| v.as_str()))
                .unwrap_or("?");
            s.push_event(EventEntry {
                ts: Local::now(),
                kind: EventKind::Accepted,
                symbol: symbol.clone(),
                detail: format!("{}  order={}", symbol, &id[..8.min(id.len())]),
            });
        }
        "order_cancelled" => {
            let symbol = val
                .get("symbol")
                .map(sym_str)
                .unwrap_or_default();
            s.push_event(EventEntry {
                ts: Local::now(),
                kind: EventKind::Cancelled,
                symbol,
                detail: "order cancelled".to_string(),
            });
        }
        "session_state" => {
            let symbol = val
                .get("symbol")
                .map(sym_str)
                .unwrap_or_default();
            let st = val
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            s.sessions.insert(symbol.clone(), st.to_string());
            s.push_event(EventEntry {
                ts: Local::now(),
                kind: EventKind::Session,
                symbol: symbol.clone(),
                detail: format!("{}  → {}", symbol, st),
            });
        }
        _ => {}
    }
}
