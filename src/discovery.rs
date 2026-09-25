use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use anyhow::{Result, bail};
use tokio::{net::UdpSocket, time};

pub fn notify(ip: &str, serial: &str, model: &str, name: &str, version: &str) -> Vec<u8> {
    [
        "NOTIFY * HTTP/1.1".into(),
        "HOST: 239.255.255.250:1900".into(),
        "Server: UPnP/1.0".into(),
        format!("Location: {ip}"),
        "NT: urn:bambulab-com:device:3dprinter:1".into(),
        format!("USN: {serial}"),
        "Cache-Control: max-age=1800".into(),
        format!("DevModel.bambu.com: {model}"),
        format!("DevName.bambu.com: {name}"),
        "DevSignal.bambu.com: -40".into(),
        "DevConnect.bambu.com: lan".into(),
        "DevBind.bambu.com: bind".into(),
        "Devseclink.bambu.com: secure".into(),
        format!("DevVersion.bambu.com: {version}"),
        "DevCap.bambu.com: 1".into(),
        "".into(),
        "".into(),
    ]
    .join("\r\n")
    .into_bytes()
}

pub async fn run() -> Result<()> {
    let ip = env::var("DISCOVERY_PRINTER_IP")?;
    let serial = env::var("DISCOVERY_PRINTER_SERIAL")?
        .trim()
        .to_ascii_uppercase();
    if serial.is_empty() {
        bail!("DISCOVERY_PRINTER_SERIAL is required")
    }
    let payload = notify(
        &ip,
        &serial,
        &env::var("DISCOVERY_PRINTER_MODEL").unwrap_or_else(|_| "H2C".into()),
        &env::var("DISCOVERY_PRINTER_NAME").unwrap_or_else(|_| "Bambu Cloud LAN Proxy".into()),
        &env::var("DISCOVERY_PRINTER_VERSION").unwrap_or_else(|_| "00.00.00.00".into()),
    );
    let target = env::var("DISCOVERY_TARGET").unwrap_or_else(|_| "255.255.255.255".into());
    let target: SocketAddr = format!("{target}:2021").parse()?;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_broadcast(true)?;
    let interval = env::var("DISCOVERY_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0_f64)
        .max(2.0);
    tracing::info!(serial, %ip, %target, "advertising LAN facade");
    loop {
        socket.send_to(&payload, target).await?;
        time::sleep(Duration::from_secs_f64(interval)).await;
    }
}
