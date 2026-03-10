mod cli;
mod config;
mod daemon;
mod pipe;
mod setup;

mod audio;
mod hotkey;
mod ipc;
mod ml_bridge;
mod session;
mod gui;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ok_claude=info".into()),
        )
        .init();

    let args = cli::Cli::parse();
    cli::run(args).await
}
