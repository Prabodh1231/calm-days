use calm_days::{store::Db, web};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let _ = std::fs::create_dir_all("data");
    let db = match Db::open("data/calm-days.db") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Could not open data/calm-days.db: {e}");
            std::process::exit(1);
        }
    };
    let shared = Arc::new(Mutex::new(db));
    web::spawn_scheduler(shared.clone());
    // Local only: never 0.0.0.0.
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", web::PORT)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Could not use port {}: {e}. Is Calm Days already running?", web::PORT);
            std::process::exit(1);
        }
    };
    println!("Calm Days is running. Open http://127.0.0.1:{} in your browser. Press Ctrl+C to stop.", web::PORT);
    if let Err(e) = axum::serve(listener, web::router(shared)).await {
        eprintln!("Server stopped: {e}");
    }
}
