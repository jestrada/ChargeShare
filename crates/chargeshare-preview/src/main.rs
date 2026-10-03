mod demo;
mod rates;
mod server;

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    server::run()
}
