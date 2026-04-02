use app_config::AppConfig;
use server::serve;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn setup_tracing(log_level: Level) {
  let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();

  tracing::subscriber::set_global_default(subscriber).expect("Setting default subscriber failed");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let config = AppConfig::load()?;
  setup_tracing(config.log_level());
  info!("Configuration loaded");
  serve(config.host(), config.port()).await?;
  Ok(())
}
