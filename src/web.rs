use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

use crate::{
    cloud_auth,
    config::{ProxyMode, env_bool, env_value},
    service::Session,
};

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

#[derive(Clone)]
struct AppState {
    session: Arc<Session>,
    access_code: Arc<str>,
}

#[derive(Debug)]
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "detail": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self(StatusCode::BAD_GATEWAY, error.to_string())
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct AdminSettings {
    mode: ProxyMode,
    allow_cancel: bool,
    cloud_print_enabled: bool,
    remote_reprint_enabled: bool,
    advertise_controls: bool,
    storage_allow_delete: bool,
    storage_allow_upload: bool,
}

impl AdminSettings {
    fn from_config(config: &crate::config::ProxyConfig) -> Result<Self> {
        Ok(Self {
            mode: config.mode,
            allow_cancel: config.allow_cancel,
            cloud_print_enabled: config.cloud_print_enabled,
            remote_reprint_enabled: config.remote_reprint_enabled,
            advertise_controls: config.advertise_controls,
            storage_allow_delete: env_bool("CLOUD_LAN_PROXY_STORAGE_ALLOW_DELETE", false)?,
            storage_allow_upload: env_bool("CLOUD_LAN_PROXY_STORAGE_ALLOW_UPLOAD", false)?,
        })
    }
}

pub fn apply_saved_settings(
    data: &std::path::Path,
    config: &mut crate::config::ProxyConfig,
) -> Result<()> {
    let Ok(bytes) = fs::read(data.join("admin-settings.json")) else {
        return Ok(());
    };
    let settings: AdminSettings = serde_json::from_slice(&bytes)?;
    if settings.mode == ProxyMode::Unsafe {
        anyhow::bail!("persisted admin settings cannot enable unsafe mode")
    }
    config.mode = settings.mode;
    config.allow_cancel = settings.allow_cancel;
    config.cloud_print_enabled = settings.cloud_print_enabled;
    config.remote_reprint_enabled = settings.remote_reprint_enabled;
    config.advertise_controls = settings.advertise_controls;
    Ok(())
}

#[derive(Deserialize)]
struct LoginRequest {
    pasted_url: String,
}

#[derive(Deserialize)]
struct EndpointRequest {
    virtual_ip: String,
    physical_ip: String,
    #[serde(default)]
    physical_access_code: String,
    enabled: bool,
}

fn authorized(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = headers
        .get("x-cloud-lan-proxy-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if state.access_code.is_empty()
        || supplied
            .as_bytes()
            .ct_eq(state.access_code.as_bytes())
            .unwrap_u8()
            != 1
    {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into()));
    }
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({"status":"ok"}))
}

async fn auth_status(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "signed_in": state.session.signed_in.load(std::sync::atomic::Ordering::Acquire),
        "cloud_connected": state.session.connected.load(std::sync::atomic::Ordering::Acquire),
        "region": state.session.region,
    }))
}

async fn overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    let registry: Vec<Value> = fs::read(state.session.data_dir().join("printers.json"))
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default();
    let metadata: HashMap<String, Value> = registry
        .into_iter()
        .filter_map(|v| Some((v.get("serial")?.as_str()?.to_ascii_uppercase(), v)))
        .collect();
    let printers = state.session.printers.read().await;
    let mut values = Vec::with_capacity(printers.len());
    for (serial, printer) in printers.iter() {
        let status = printer.status().await;
        let entry = metadata.get(serial);
        values.push(json!({
            "serial": serial,
            "name": entry.and_then(|v|v.get("name")).and_then(Value::as_str).unwrap_or(""),
            "model": entry.and_then(|v|v.get("machine_model")).and_then(Value::as_str).unwrap_or(""),
            "virtual_ip": entry.and_then(|v|v.get("virtual_ip")).and_then(Value::as_str).unwrap_or(""),
            "physical_ip": entry.and_then(|v|v.get("ip")).and_then(Value::as_str).unwrap_or(""),
            "physical_access_configured": entry.and_then(|v|v.get("access_code")).and_then(Value::as_str).is_some_and(|v|!v.is_empty()),
            "endpoint_enabled": entry.and_then(|v|v.get("endpoint_enabled")).and_then(Value::as_bool).unwrap_or(true),
            "online": status.online,
            "state": status.state,
            "secure_control": status.secure_control,
        }));
    }
    values.sort_by(|a, b| a["serial"].as_str().cmp(&b["serial"].as_str()));
    let plugin_ok = state.session.host.failure().await.is_none();
    Json(json!({
        "healthy": plugin_ok,
        "signed_in": state.session.signed_in.load(std::sync::atomic::Ordering::Acquire),
        "cloud_connected": state.session.connected.load(std::sync::atomic::Ordering::Acquire),
        "region": state.session.region,
        "printers": values,
    }))
    .pipe(Ok)
}

async fn put_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(serial): AxumPath<String>,
    Json(request): Json<EndpointRequest>,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    state
        .session
        .configure_endpoint(
            &serial,
            &request.virtual_ip,
            &request.physical_ip,
            &request.physical_access_code,
            request.enabled,
        )
        .await
        .map_err(|error| ApiError(StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(Json(json!({"ok":true,"certificate_refresh_required":true})))
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdminSettings>, ApiError> {
    authorized(&state, &headers)?;
    let path = state.session.data_dir().join("admin-settings.json");
    let settings = fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(
            AdminSettings::from_config(&state.session.facade.config().await)
                .map_err(ApiError::from)?,
        );
    Ok(Json(settings))
}

async fn put_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(settings): Json<AdminSettings>,
) -> Result<Json<AdminSettings>, ApiError> {
    authorized(&state, &headers)?;
    if settings.mode == ProxyMode::Unsafe {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "unsafe mode cannot be enabled from the admin console".into(),
        ));
    }
    state
        .session
        .facade
        .update_policy(
            settings.mode,
            settings.allow_cancel,
            settings.cloud_print_enabled,
            settings.remote_reprint_enabled,
            settings.advertise_controls,
        )
        .await;
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let path = state.session.data_dir().join("admin-settings.json");
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, bytes)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::rename(temporary, path)
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(settings))
}

async fn login_url(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    Ok(Json(
        json!({"url": cloud_auth::signin_url(&state.session.region)?}),
    ))
}

async fn login_paste(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    if request.pasted_url.len() > 20_000 {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "callback URL is too long".into(),
        ));
    }
    cloud_auth::login(
        &state.session.host,
        &state.session.region,
        &request.pasted_url,
    )
    .await?;
    state.session.connect().await?;
    Ok(Json(json!({"ok":true})))
}

async fn export_ca(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorized(&state, &headers)?;
    let path = PathBuf::from(env_value(
        "PROXY_CA_EXPORT_FILE",
        "/proxy-ca/bambu-cloud-lan-proxy-ca.crt",
    ));
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "proxy CA is not available".into()))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/x-pem-file")
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"bambu-cloud-lan-proxy-ca.crt\"",
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .unwrap())
}

fn text_field(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

async fn current_settings(state: &AppState) -> Result<AdminSettings, ApiError> {
    let path = state.session.data_dir().join("admin-settings.json");
    Ok(fs::read(path)
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(
            AdminSettings::from_config(&state.session.facade.config().await)
                .map_err(ApiError::from)?,
        ))
}

async fn storage_capabilities(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    let settings = current_settings(&state).await?;
    Ok(Json(
        json!({"ability":true,"list":true,"download":true,"subfile":true,
        "upload":settings.storage_allow_upload,"delete":settings.storage_allow_delete,
        "notifications":"revision","time":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()}),
    ))
}

async fn storage_operation(
    State(state): State<AppState>,
    AxumPath(operation): AxumPath<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    authorized(&state, &headers)?;
    let serial = text_field(&body, "serial").to_ascii_uppercase();
    let config = state.session.facade.config().await;
    if operation != "discard" && (serial.is_empty() || !config.permits_printer(&serial)) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "printer is not allowed".into(),
        ));
    }
    let settings = current_settings(&state).await?;
    let cache_root: PathBuf = env_value("CLOUD_LAN_PROXY_REMOTE_CACHE_DIR", "/remote-cache").into();
    tokio::fs::create_dir_all(&cache_root)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let timeout = Duration::from_secs(21 * 60);
    let mut result = match operation.as_str() {
        "list" => state.session.host.call("storage.list", json!({"dev_id":serial,"type":text_field(&body,"type"),"storage":text_field(&body,"storage")}), Duration::from_secs(60)).await?,
        "ability" => state.session.host.call("storage.ability", json!({"dev_id":serial,"api_version":body.get("api_version").and_then(Value::as_u64).unwrap_or(2)}), Duration::from_secs(60)).await?,
        "download" | "subfile" => {
            if operation == "download" && text_field(&body,"path").is_empty() && text_field(&body,"name").is_empty() {
                return Err(ApiError(StatusCode::BAD_REQUEST,"path or name is required".into()));
            }
            let key = uuid::Uuid::new_v4().simple().to_string();
            let target = cache_root.join(&key);
            let default_mb = if operation == "subfile" { "256" } else { "4096" };
            let variable = if operation == "subfile" { "CLOUD_LAN_PROXY_REMOTE_SUBFILE_MAX_MB" } else { "CLOUD_LAN_PROXY_REMOTE_MAX_MB" };
            let max_bytes = env_value(variable,default_mb).parse::<u64>().unwrap_or(256) * 1024 * 1024;
            let params = if operation == "subfile" {
                json!({"dev_id":serial,"request":body.get("request").cloned().unwrap_or(Value::Null),"target_path":target,"max_bytes":max_bytes})
            } else {
                json!({"dev_id":serial,"path":text_field(&body,"path"),"name":text_field(&body,"name"),"target_path":target,"max_bytes":max_bytes})
            };
            match state.session.host.call(&format!("storage.{operation}"), params, timeout).await {
                Ok(value) => json!({"cache_key":key,"size":value.get("size").and_then(Value::as_u64).unwrap_or(0)}),
                Err(error) => { let _=tokio::fs::remove_file(target).await; return Err(error.into()) }
            }
        },
        "delete" => {
            if !settings.storage_allow_delete { return Err(ApiError(StatusCode::FORBIDDEN,"remote delete is disabled".into())); }
            let paths = body.get("paths").and_then(Value::as_array).context("invalid delete request").map_err(ApiError::from)?;
            if paths.is_empty() || paths.len() > 100 { return Err(ApiError(StatusCode::BAD_REQUEST,"invalid delete request".into())); }
            state.session.host.call("storage.delete",json!({"dev_id":serial,"paths":paths}),Duration::from_secs(90)).await?
        },
        "upload" => {
            if !settings.storage_allow_upload { return Err(ApiError(StatusCode::FORBIDDEN,"remote upload is disabled".into())); }
            let source = tokio::fs::canonicalize(text_field(&body,"source_path")).await.map_err(|_| ApiError(StatusCode::BAD_REQUEST,"upload source is unavailable".into()))?;
            let staging = tokio::fs::canonicalize(&config.staging_dir).await.map_err(|_| ApiError(StatusCode::BAD_REQUEST,"staging directory is unavailable".into()))?;
            if !source.starts_with(staging) || !tokio::fs::metadata(&source).await.map(|m|m.is_file()).unwrap_or(false) {
                return Err(ApiError(StatusCode::BAD_REQUEST,"upload source is outside staging".into()));
            }
            state.session.host.call("storage.upload",json!({"dev_id":serial,"storage":text_field(&body,"storage"),"path":text_field(&body,"path"),"source_path":source,"max_bytes":env_value("CLOUD_LAN_PROXY_REMOTE_MAX_MB","4096").parse::<u64>().unwrap_or(4096)*1024*1024}),timeout).await?
        },
        "discard" => {
            let key = text_field(&body,"cache_key");
            if key.len()!=32 || !key.bytes().all(|v|v.is_ascii_alphanumeric()) { return Err(ApiError(StatusCode::BAD_REQUEST,"invalid cache key".into())); }
            let _=tokio::fs::remove_file(cache_root.join(key)).await;
            return Ok(Json(json!({"discarded":true})));
        },
        _ => return Err(ApiError(StatusCode::NOT_FOUND,"unknown storage operation".into())),
    };
    if matches!(operation.as_str(), "delete" | "upload") {
        result["revision"] = 1.into();
    } else if operation == "ability" {
        result["revision"] = 0.into();
    }
    Ok(Json(result))
}

async fn asset(AxumPath(path): AxumPath<String>) -> Response {
    serve_asset(&path)
}
async fn fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty()
        || !path
            .rsplit('/')
            .next()
            .is_some_and(|part| part.contains('.'))
    {
        serve_asset("index.html")
    } else {
        serve_asset(path)
    }
}

fn serve_asset(path: &str) -> Response {
    let asset = Assets::get(path);
    let Some(asset) = asset else {
        return (StatusCode::NOT_FOUND, "admin UI is not built").into_response();
    };
    let mime = match path.rsplit('.').next() {
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "text/html; charset=utf-8",
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(
            header::CACHE_CONTROL,
            if path == "index.html" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            },
        )
        .body(Body::from(asset.data.into_owned()))
        .unwrap()
}

pub async fn serve(session: Arc<Session>) -> Result<()> {
    let access_code: Arc<str> = env_value("CLOUD_LAN_PROXY_ACCESS_CODE", "").into();
    if access_code.is_empty() {
        anyhow::bail!("CLOUD_LAN_PROXY_ACCESS_CODE is required")
    }
    let state = AppState {
        session,
        access_code,
    };
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/cloud/auth/status", get(auth_status))
        .route("/api/cloud/auth/url", get(login_url))
        .route("/api/cloud/auth/paste", post(login_paste))
        .route("/api/admin/overview", get(overview))
        .route(
            "/api/admin/printers/{serial}/endpoint",
            axum::routing::put(put_endpoint),
        )
        .route("/api/admin/settings", get(get_settings).put(put_settings))
        .route("/api/admin/ca", get(export_ca))
        .route(
            "/internal/cloud-lan-proxy/storage/capabilities",
            get(storage_capabilities),
        )
        .route(
            "/internal/cloud-lan-proxy/storage/{operation}",
            post(storage_operation),
        )
        .route("/assets/{*path}", get(asset))
        .fallback(fallback)
        .with_state(state);
    let address: SocketAddr = env_value("PROXY_HTTP_BIND", "0.0.0.0:4844").parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "admin server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
