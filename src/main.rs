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
    // First run: fill the board with an example so the first screen is never blank.
    if db.meta("seeded").is_none() {
        if db.list(true).map(|v| v.is_empty()).unwrap_or(false) {
            let _ = calm_days::ops::load_sample(&db, calm_days::store::now_local());
        }
        let _ = db.set_meta("seeded", "1");
    }
    // Start Ollama (if needed) and load the model in the background, so the AI is ready for the first sentence.
    let st = calm_days::ai::load_settings(&db);
    if st.on {
        std::thread::spawn(move || calm_days::ai::warm(&st));
    }
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
