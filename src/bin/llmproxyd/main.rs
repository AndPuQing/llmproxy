use clap::Parser;
use clap_verbosity_flag::Verbosity;
use std::net::{IpAddr, SocketAddr};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(flatten)]
    verbosity: Verbosity,

    #[arg(short, long, default_value = "11450")]
    port: u16,

    #[arg(long, default_value = "0.0.0.0")]
    host: IpAddr,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_max_level(cli.verbosity)
        .init();

    let addr = SocketAddr::new(cli.host, cli.port);
    llmproxy::server::run(addr).await;
}
