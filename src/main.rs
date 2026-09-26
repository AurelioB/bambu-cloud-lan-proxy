mod bridge;
mod camera;
mod cloud_auth;
mod cloud_printer;
mod commands;
mod config;
mod discovery;
mod endpoint;
mod ftps;
mod local_status;
mod native_files;
mod plugin_install;
mod plugin_rpc;
mod policy;
mod print_jobs;
mod reports;
mod service;
mod tcp_relay;
mod tls;
mod web;

use anyhow::{Result, anyhow};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Camera,
    Service,
    Discovery,
    Endpoint,
    EndpointAddresses,
    EndpointManager,
    EndpointOptional,
    Ftps,
    LocalStatus,
    NativeFiles,
    TcpRelay,
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow!("failed to install the ring TLS crypto provider"))?;
    let log_filter = std::env::var("RUST_LOG")
        .or_else(|_| std::env::var("LOG_LEVEL"))
        .unwrap_or_else(|_| "bambu_cloud_lan_proxy=info".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_new(log_filter)?)
        .with_target(false)
        .init();

    match Cli::parse().command.unwrap_or(Command::Service) {
        Command::Camera => camera::run().await,
        Command::Service => service::run().await,
        Command::Discovery => discovery::run().await,
        Command::Endpoint => endpoint::run_endpoint().await,
        Command::EndpointAddresses => endpoint::configure_addresses().await,
        Command::EndpointManager => endpoint::run_manager().await,
        Command::EndpointOptional => endpoint::run_optional().await,
        Command::Ftps => ftps::run().await,
        Command::LocalStatus => local_status::run().await,
        Command::NativeFiles => native_files::run().await,
        Command::TcpRelay => tcp_relay::run().await,
    }
}
