//! Diagnostic: print the raw event stream from the player, and what the sync
//! engine makes of it. `cargo run --example pump_dump -- [seconds]`.
use std::time::{Duration, Instant};

use terminal_lyrics::player::Session;
use terminal_lyrics::sync::SyncEngine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let session = Session::open().await?;
    let name = session.resolve(None).await?;
    let handle = session.connect(&name).await?;
    let (mut rx, _pump) = handle.spawn(Duration::from_secs(1));

    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5);

    let start = Instant::now();
    let mut engine = SyncEngine::new(0, Duration::from_millis(250), start);
    while start.elapsed() < Duration::from_secs(seconds) {
        tokio::select! {
            Some(ev) = rx.recv() => {
                let now = Instant::now();
                let change = engine.apply(ev.clone(), now);
                println!(
                    "{:>5.2}s  {:?}\n        -> change={:?} pos={:.2} playing={}",
                    start.elapsed().as_secs_f64(),
                    ev,
                    change,
                    engine.lyric_position(now),
                    engine.is_playing()
                );
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                println!("{:>5.2}s  (idle) pos={:.2}", start.elapsed().as_secs_f64(), engine.lyric_position(Instant::now()));
            }
        }
    }
    Ok(())
}
