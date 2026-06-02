use crate::app::MonitorState;
use rand::Rng;
use reqwest::Client;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

pub async fn fire_swarm(
    client: Client,
    base_url: String,
    symbol: String,
    count: usize,
    state: Arc<Mutex<MonitorState>>,
) {
    let sem = Arc::new(Semaphore::new(20));
    let mut handles = Vec::new();

    for _ in 0..count {
        let client = client.clone();
        let base_url = base_url.clone();
        let symbol = symbol.clone();
        let sem = sem.clone();
        let state = state.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();

            // Generate random values before any await point (ThreadRng is not Send)
            let (side, price, qty, client_id) = {
                let mut rng = rand::thread_rng();
                let side = if rng.gen_bool(0.5) { "Buy" } else { "Sell" };
                // prices clustered around 100 with ±5% spread to encourage crossing
                let base: f64 = 100.0;
                let price_f = base + rng.gen_range(-5.0_f64..5.0_f64);
                let price = format!("{:.2}", price_f);
                let qty = rng.gen_range(10_u32..=500_u32).to_string();
                let clients = ["demo", "trader1", "trader2"];
                let client_id = clients[rng.gen_range(0..clients.len())].to_string();
                (side, price, qty, client_id)
            };

            let body = json!({
                "symbol": symbol,
                "side": side,
                "order_type": { "type": "Limit", "price": price },
                "quantity": qty,
                "time_in_force": "GoodTillCanceled",
                "client_id": client_id,
            });

            let t0 = std::time::Instant::now();
            let result = client
                .post(format!("{}/v1/orders", base_url))
                .json(&body)
                .send()
                .await;
            let latency_ms = t0.elapsed().as_secs_f64() * 1000.0;

            if let Ok(resp) = result {
                if resp.status().is_success() {
                    let mut s = state.lock().unwrap();
                    s.metrics.record_submit(latency_ms);
                }
            }
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.await;
    }
}
