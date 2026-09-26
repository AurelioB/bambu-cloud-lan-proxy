use crate::{camera, config::env_value};
use anyhow::{Result, bail};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, TlsConfiguration, Transport};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

pub async fn run() -> Result<()> {
    let registry = PathBuf::from(env_value(
        "LOCAL_STATUS_GATEWAY_REGISTRY",
        "/data/printers.json",
    ));
    let source = camera::load_source(&registry, &env_value("LOCAL_STATUS_PRINTER_SERIAL", ""))?;
    let upstream_ip = env_value("LOCAL_STATUS_UPSTREAM_IP", "");
    if upstream_ip.is_empty() {
        bail!("LOCAL_STATUS_UPSTREAM_IP is required")
    }
    let upstream_port = env_value("LOCAL_STATUS_UPSTREAM_PORT", "8883").parse::<u16>()?;
    let state = PathBuf::from(env_value("LOCAL_STATUS_STATE_DIR", "/state"));
    fs::create_dir_all(&state)?;
    let heartbeat = state.join(format!("{}.local", source.serial));
    let fingerprint = camera::resolve_fingerprint(
        &upstream_ip,
        upstream_port,
        &format!("{}-mqtt", source.serial),
        &state,
        &env_value("LOCAL_STATUS_TLS_FINGERPRINT", ""),
    )
    .await?;
    let mut downstream_options = MqttOptions::new(
        format!("bambu-local-status-out-{}", source.serial),
        env_value("LOCAL_STATUS_BROKER_HOST", "mqtt"),
        env_value("LOCAL_STATUS_BROKER_PORT", "1883").parse::<u16>()?,
    );
    downstream_options.set_keep_alive(Duration::from_secs(30));
    downstream_options.set_max_packet_size(2 * 1024 * 1024, 2 * 1024 * 1024);
    let (downstream, mut downstream_loop) = AsyncClient::new(downstream_options, 100);
    let mut upstream_options = MqttOptions::new(
        format!("bambu-local-status-in-{}", source.serial),
        &upstream_ip,
        upstream_port,
    );
    upstream_options
        .set_keep_alive(Duration::from_secs(30))
        .set_max_packet_size(2 * 1024 * 1024, 2 * 1024 * 1024)
        .set_credentials("bblp", source.access_code);
    upstream_options.set_transport(Transport::tls_with_config(TlsConfiguration::Rustls(
        Arc::new(crate::tls::pinned_client_config(&fingerprint)?),
    )));
    let (upstream, mut upstream_loop) = AsyncClient::new(upstream_options, 100);
    upstream
        .subscribe(format!("device/{}/report", source.serial), QoS::AtMostOnce)
        .await?;
    let topic = format!("device/{}/report", source.serial);
    tracing::info!(serial=%source.serial,"local read-only telemetry enabled");
    let result = loop {
        tokio::select! {
            event=upstream_loop.poll()=>match event{Ok(Event::Incoming(Incoming::Publish(message)))=>{if message.topic==topic&&serde_json::from_slice::<Value>(&message.payload).is_ok(){downstream.publish(&message.topic,QoS::AtMostOnce,false,message.payload).await?;let temporary=state.join(format!(".{}.tmp",source.serial));tokio::fs::write(&temporary,format!("{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())).await?;tokio::fs::rename(temporary,&heartbeat).await?}},Ok(_)=>{},Err(error)=>{tracing::warn!(%error,"local telemetry unavailable; cloud fallback remains active");tokio::time::sleep(Duration::from_secs(2)).await;}},
            event=downstream_loop.poll()=>if let Err(error)=event{tracing::warn!(%error,"local MQTT broker unavailable");tokio::time::sleep(Duration::from_secs(2)).await;},
            signal=tokio::signal::ctrl_c()=>{signal?;break Ok(())}
        }
    };
    let _ = tokio::fs::remove_file(heartbeat).await;
    result
}
