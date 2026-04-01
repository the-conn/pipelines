use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn setup_tracing() {
  let subscriber = FmtSubscriber::builder()
    .with_max_level(Level::INFO)
    .with_env_filter("")
    .finish();

  tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  setup_tracing();

  Ok(())
}
