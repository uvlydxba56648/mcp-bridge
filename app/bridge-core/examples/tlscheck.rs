#[tokio::main]
async fn main() {
    for url in ["https://one.one.one.one", "https://api.github.com"] {
        let t0 = std::time::Instant::now();
        match reqwest::get(url).await {
            Ok(r) => println!("{} -> {} ({} ms)", url, r.status(), t0.elapsed().as_millis()),
            Err(e) => println!("{} -> ERR {}", url, e),
        }
    }
}
