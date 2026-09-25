use std::env;

use anyhow::Result;
use tokio::{
    io,
    net::{TcpListener, TcpStream},
};

pub async fn run() -> Result<()> {
    let host = env::var("TCP_RELAY_LISTEN_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port = env::var("TCP_RELAY_LISTEN_PORT").unwrap_or_else(|_| "8322".into());
    let upstream_host =
        env::var("TCP_RELAY_UPSTREAM_HOST").unwrap_or_else(|_| "host.docker.internal".into());
    let upstream_port = env::var("TCP_RELAY_UPSTREAM_PORT").unwrap_or_else(|_| "8322".into());
    let listener = TcpListener::bind(format!("{host}:{port}")).await?;
    loop {
        let (mut client, _) = listener.accept().await?;
        let upstream = format!("{upstream_host}:{upstream_port}");
        tokio::spawn(async move {
            let Ok(mut remote) = TcpStream::connect(upstream).await else {
                return;
            };
            let _ = io::copy_bidirectional(&mut client, &mut remote).await;
        });
    }
}
