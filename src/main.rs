use anyhow::Result;
use clap::Parser;
use pyaichat::cli::CliArgs;

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    pyaichat::run(args).await
}
