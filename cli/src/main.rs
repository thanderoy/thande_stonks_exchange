mod app;
mod client;
mod swarm;
mod ui;

use app::{CancelForm, InputMode, MonitorState, OrderForm};
use crossterm::{
    event::{self, Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use reqwest::Client;
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};

#[tokio::main]
async fn main() {
    let base_url =
        std::env::var("EXCHANGE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let symbols_env = std::env::var("SYMBOLS").unwrap_or_else(|_| "AAPL".to_string());
    let symbols: Vec<String> = symbols_env
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let state = Arc::new(Mutex::new(MonitorState::new(base_url.clone(), symbols)));

    tokio::spawn(client::poll_books(state.clone()));
    tokio::spawn(client::poll_sessions(state.clone()));
    tokio::spawn(client::subscribe_ws(state.clone()));
    tokio::spawn(auto_stream_loop(state.clone()));

    enable_raw_mode().unwrap();
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    let result = run_app(&mut terminal, state, base_url).await;

    disable_raw_mode().unwrap();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap();
    terminal.show_cursor().unwrap();

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
}

async fn auto_stream_loop(state: Arc<Mutex<MonitorState>>) {
    let http = Client::new();
    loop {
        let (active, interval_ms, sym, url) = {
            let s = state.lock().unwrap();
            (
                s.auto_stream,
                s.auto_interval_ms,
                s.selected_symbol().to_string(),
                s.base_url.clone(),
            )
        };

        if active && !sym.is_empty() {
            tokio::spawn(swarm::fire_swarm(http.clone(), url, sym, 1, state.clone()));
        }

        tokio::time::sleep(Duration::from_millis(if active { interval_ms } else { 100 })).await;
    }
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: Arc<Mutex<MonitorState>>,
    base_url: String,
) -> io::Result<()> {
    let http = Client::new();
    let mut render_tick = tokio::time::interval(Duration::from_millis(100));
    let mut event_stream = EventStream::new();

    loop {
        tokio::select! {
            _ = render_tick.tick() => {
                terminal.draw(|f| {
                    let s = state.lock().unwrap();
                    ui::render(f, &s);
                })?;
            }
            maybe_event = event_stream.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => {
                        if handle_key(key, &state, &base_url, &http).await {
                            return Ok(());
                        }
                    }
                    Some(Err(e)) => return Err(e),
                    None => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

async fn handle_key(
    key: event::KeyEvent,
    state: &Arc<Mutex<MonitorState>>,
    base_url: &str,
    http: &Client,
) -> bool {
    let mode = state.lock().unwrap().mode.clone();

    match mode {
        InputMode::Normal => handle_normal(key, state, base_url, http).await,
        InputMode::SubmitForm => handle_submit_form(key, state, base_url, http).await,
        InputMode::CancelInput => handle_cancel_form(key, state, base_url, http).await,
    }
}

async fn handle_normal(
    key: event::KeyEvent,
    state: &Arc<Mutex<MonitorState>>,
    base_url: &str,
    http: &Client,
) -> bool {
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => return true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,

        KeyCode::Char('s') => {
            let (sym, client_id) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), "demo".to_string())
            };
            let mut s = state.lock().unwrap();
            s.form = OrderForm {
                symbol: sym,
                side: "Buy".to_string(),
                price: "100.00".to_string(),
                qty: "100".to_string(),
                client: client_id,
                active_field: 1,
                error: None,
            };
            s.mode = InputMode::SubmitForm;
        }

        KeyCode::Char('c') => {
            let sym = state.lock().unwrap().selected_symbol().to_string();
            let mut s = state.lock().unwrap();
            s.cancel_form = CancelForm {
                order_id: String::new(),
                symbol: sym,
                side: "Buy".to_string(),
                error: None,
            };
            s.mode = InputMode::CancelInput;
        }

        KeyCode::Char('H') => {
            let (sym, url) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), s.base_url.clone())
            };
            let http = http.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let body = serde_json::json!({"reason": "manual", "threshold_pct": "0"});
                let _ = http
                    .post(format!("{}/v1/session/{}/halt", url, sym))
                    .json(&body)
                    .send()
                    .await;
                state.lock().unwrap().push_log(format!("Sent halt for {}", sym));
            });
        }

        KeyCode::Char('o') => {
            let (sym, url) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), s.base_url.clone())
            };
            let http = http.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let _ = http
                    .post(format!("{}/v1/session/{}/open", url, sym))
                    .send()
                    .await;
                state.lock().unwrap().push_log(format!("Sent open for {}", sym));
            });
        }

        KeyCode::Char('r') => {
            let (sym, url) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), s.base_url.clone())
            };
            let http = http.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let _ = http
                    .post(format!("{}/v1/session/{}/resume", url, sym))
                    .send()
                    .await;
                state.lock().unwrap().push_log(format!("Sent resume for {}", sym));
            });
        }

        KeyCode::Char('w') => {
            let (sym, url) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), s.base_url.clone())
            };
            tokio::spawn(swarm::fire_swarm(http.clone(), url, sym, 10, state.clone()));
        }

        KeyCode::Char('W') => {
            let (sym, url) = {
                let s = state.lock().unwrap();
                (s.selected_symbol().to_string(), s.base_url.clone())
            };
            tokio::spawn(swarm::fire_swarm(http.clone(), url, sym, 100, state.clone()));
        }

        KeyCode::Char('a') => {
            let mut s = state.lock().unwrap();
            s.auto_stream = !s.auto_stream;
            let msg = if s.auto_stream {
                format!("Auto-stream ON  ({}ms interval, +/- to adjust)", s.auto_interval_ms)
            } else {
                "Auto-stream OFF".to_string()
            };
            s.push_log(msg);
        }

        KeyCode::Char('+') | KeyCode::Char('=') => {
            let mut s = state.lock().unwrap();
            s.auto_interval_ms = (s.auto_interval_ms / 2).max(50);
            let ms = s.auto_interval_ms;
            s.push_log(format!("Auto-stream interval → {}ms", ms));
        }

        KeyCode::Char('-') => {
            let mut s = state.lock().unwrap();
            s.auto_interval_ms = (s.auto_interval_ms * 2).min(5000);
            let ms = s.auto_interval_ms;
            s.push_log(format!("Auto-stream interval → {}ms", ms));
        }

        KeyCode::Tab | KeyCode::Down => {
            let mut s = state.lock().unwrap();
            let len = s.symbols.len();
            if len > 0 {
                s.selected = (s.selected + 1) % len;
            }
        }

        KeyCode::Up => {
            let mut s = state.lock().unwrap();
            let len = s.symbols.len();
            if len > 0 {
                s.selected = (s.selected + len - 1) % len;
            }
        }

        _ => {}
    }
    false
}

async fn handle_submit_form(
    key: event::KeyEvent,
    state: &Arc<Mutex<MonitorState>>,
    base_url: &str,
    http: &Client,
) -> bool {
    match key.code {
        KeyCode::Esc => {
            state.lock().unwrap().mode = InputMode::Normal;
        }

        KeyCode::Tab => {
            let mut s = state.lock().unwrap();
            s.form.active_field = (s.form.active_field + 1) % 4;
        }

        KeyCode::Char(' ') => {
            let mut s = state.lock().unwrap();
            if s.form.active_field == 0 {
                s.form.side = if s.form.side == "Buy" {
                    "Sell".to_string()
                } else {
                    "Buy".to_string()
                };
            }
        }

        KeyCode::Backspace => {
            let mut s = state.lock().unwrap();
            match s.form.active_field {
                1 => {
                    s.form.price.pop();
                }
                2 => {
                    s.form.qty.pop();
                }
                3 => {
                    s.form.client.pop();
                }
                _ => {}
            }
        }

        KeyCode::Char(c) => {
            let mut s = state.lock().unwrap();
            match s.form.active_field {
                1 => s.form.price.push(c),
                2 => s.form.qty.push(c),
                3 => s.form.client.push(c),
                _ => {}
            }
        }

        KeyCode::Enter => {
            let (sym, side, price, qty, client, url) = {
                let s = state.lock().unwrap();
                (
                    s.form.symbol.clone(),
                    s.form.side.clone(),
                    s.form.price.clone(),
                    s.form.qty.clone(),
                    s.form.client.clone(),
                    s.base_url.clone(),
                )
            };

            let body = serde_json::json!({
                "symbol": sym,
                "side": side,
                "order_type": { "type": "Limit", "price": price },
                "quantity": qty,
                "time_in_force": "GoodTillCanceled",
                "client_id": client,
            });

            let http = http.clone();
            let state = state.clone();
            tokio::spawn(async move {
                let t0 = std::time::Instant::now();
                match http
                    .post(format!("{}/v1/orders", url))
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;
                        let mut s = state.lock().unwrap();
                        s.metrics.record_submit(latency_ms);
                        s.mode = InputMode::Normal;
                        s.push_log("Order submitted".to_string());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        let mut s = state.lock().unwrap();
                        s.form.error = Some(format!("{}: {}", status, body));
                    }
                    Err(e) => {
                        let mut s = state.lock().unwrap();
                        s.form.error = Some(e.to_string());
                    }
                }
            });
        }

        _ => {}
    }
    false
}

async fn handle_cancel_form(
    key: event::KeyEvent,
    state: &Arc<Mutex<MonitorState>>,
    base_url: &str,
    http: &Client,
) -> bool {
    match key.code {
        KeyCode::Esc => {
            state.lock().unwrap().mode = InputMode::Normal;
        }

        KeyCode::Backspace => {
            let mut s = state.lock().unwrap();
            s.cancel_form.order_id.pop();
        }

        KeyCode::Char(c) => {
            let mut s = state.lock().unwrap();
            s.cancel_form.order_id.push(c);
        }

        KeyCode::Enter => {
            let (id, sym, side, url) = {
                let s = state.lock().unwrap();
                (
                    s.cancel_form.order_id.clone(),
                    s.cancel_form.symbol.clone(),
                    s.cancel_form.side.clone(),
                    s.base_url.clone(),
                )
            };
            let http = http.clone();
            let state = state.clone();
            tokio::spawn(async move {
                match http
                    .delete(format!(
                        "{}/v1/orders/{}?symbol={}&side={}",
                        url, id, sym, side
                    ))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let mut s = state.lock().unwrap();
                        s.mode = InputMode::Normal;
                        s.push_log(format!("Cancelled {}", &id[..8.min(id.len())]));
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        let mut s = state.lock().unwrap();
                        s.cancel_form.error = Some(format!("{}: {}", status, body));
                    }
                    Err(e) => {
                        let mut s = state.lock().unwrap();
                        s.cancel_form.error = Some(e.to_string());
                    }
                }
            });
        }
        _ => {}
    }
    false
}
