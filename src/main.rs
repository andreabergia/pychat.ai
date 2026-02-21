use anyhow::Result;
use clap::Parser;
use pychat_ai::cli::CliArgs;

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();
    pychat_ai::run(args).await
}
