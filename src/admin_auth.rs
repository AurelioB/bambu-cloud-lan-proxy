//! Browser identity is deliberately separate from the LAN/service access code.
//! All private HTTP routes pass through guard(), including future routes.
use anyhow::{Context, Result, bail};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenResponse,
    core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use url::Url;

use crate::config::env_value;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    AccessCode,
    Local,
    Oidc,
    Both,
}

#[derive(Clone)]
pub struct Config {
    pub mode: Mode,
    pub public_url: Option<Url>,
    pub username: String,
    password_hash: String,
    issuer: Option<IssuerUrl>,
    client_id: String,
    client_secret: String,
    subjects: HashSet<String>,
    session_seconds: u64,
}

fn secret(name: &str) -> Result<String> {
    let file = env_value(&format!("{name}_FILE"), "");
    let value = env_value(name, "");
    if !file.is_empty() && !value.is_empty() {
        bail!("set either {name} or {name}_FILE, not both");
    }
    if file.is_empty() {
        Ok(value)
    } else {
        Ok(std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {name}_FILE"))?
            .trim()
            .to_owned())
    }
}
fn secure_url(url: &Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && (url.host_str() == Some("localhost")
                || url.host_str().is_some_and(|h| {
                    h.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
                })))
}
impl Config {
    pub fn from_env() -> Result<Self> {
        Self::parse(
            &env_value("ADMIN_AUTH_MODE", "access_code"),
            &env_value("ADMIN_PUBLIC_URL", ""),
            env_value("ADMIN_USERNAME", ""),
            secret("ADMIN_PASSWORD_HASH")?,
            env_value("ADMIN_OIDC_ISSUER", ""),
            env_value("ADMIN_OIDC_CLIENT_ID", ""),
            secret("ADMIN_OIDC_CLIENT_SECRET")?,
            env_value("ADMIN_OIDC_ALLOWED_SUBJECTS", ""),
            env_value("ADMIN_SESSION_SECONDS", "3600")
                .parse()
                .context("invalid ADMIN_SESSION_SECONDS")?,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn parse(
        mode: &str,
        public_url: &str,
        username: String,
        password_hash: String,
        issuer: String,
        client_id: String,
        client_secret: String,
        subjects: String,
        session_seconds: u64,
    ) -> Result<Self> {
        let mode = match mode {
            "access_code" => Mode::AccessCode,
            "local" => Mode::Local,
            "oidc" => Mode::Oidc,
            "both" => Mode::Both,
            _ => bail!(
                "ADMIN_AUTH_MODE must be access_code, local, oidc, or both; anonymous administration is not supported"
            ),
        };
        let public_url = if public_url.is_empty() {
            None
        } else {
            let url = Url::parse(public_url).context("invalid ADMIN_PUBLIC_URL")?;
            if !secure_url(&url)
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                bail!(
                    "ADMIN_PUBLIC_URL must be an HTTPS origin (HTTP allowed only on loopback), without a path or credentials"
                );
            }
            Some(url)
        };
        if mode != Mode::AccessCode && public_url.is_none() {
            bail!("ADMIN_PUBLIC_URL is required with local or OIDC authentication");
        }
        if matches!(mode, Mode::Local | Mode::Both) {
            if username.trim().is_empty() || username.len() > 128 {
                bail!("ADMIN_USERNAME must contain 1–128 characters");
            }
            validate_password_hash(&password_hash)?;
        }
        let subjects: HashSet<_> = subjects
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        let issuer = if matches!(mode, Mode::Oidc | Mode::Both) {
            let url =
                Url::parse(&issuer).context("ADMIN_OIDC_ISSUER is required and must be a URL")?;
            if !secure_url(&url)
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                bail!("OIDC issuer must use HTTPS (HTTP allowed only on loopback)");
            }
            if client_id.is_empty() || subjects.is_empty() {
                bail!(
                    "ADMIN_OIDC_CLIENT_ID and ADMIN_OIDC_ALLOWED_SUBJECTS are required; access is deny-by-default"
                );
            }
            Some(IssuerUrl::new(issuer).context("invalid OIDC issuer")?)
        } else {
            None
        };
        if !(300..=86400).contains(&session_seconds) {
            bail!("ADMIN_SESSION_SECONDS must be between 300 and 86400");
        }
        Ok(Self {
            mode,
            public_url,
            username,
            password_hash,
            issuer,
            client_id,
            client_secret,
            subjects,
            session_seconds,
        })
    }
    fn secure(&self) -> bool {
        self.public_url
            .as_ref()
            .is_some_and(|url| url.scheme() == "https")
    }
    fn session_cookie(&self) -> &'static str {
        if self.secure() {
            "__Host-bambu-session"
        } else {
            "bambu-session"
        }
    }
    fn flow_cookie(&self) -> &'static str {
        if self.secure() {
            "__Host-bambu-login"
        } else {
            "bambu-login"
        }
    }
    fn callback(&self) -> String {
        format!(
            "{}api/auth/oidc/callback",
            self.public_url.as_ref().expect("validated public URL")
        )
    }
}

fn validate_password_hash(value: &str) -> Result<()> {
    let hash =
        PasswordHash::new(value).map_err(|_| anyhow::anyhow!("invalid ADMIN_PASSWORD_HASH"))?;
    if hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(19)
        || hash.salt.is_none()
        || hash.hash.is_none()
        || hash.params.get_decimal("m").unwrap_or(0) < 19456
        || hash.params.get_decimal("m").unwrap_or(u32::MAX) > 262144
        || !(2..=10).contains(&hash.params.get_decimal("t").unwrap_or(0))
        || !(1..=8).contains(&hash.params.get_decimal("p").unwrap_or(0))
    {
        bail!(
            "ADMIN_PASSWORD_HASH must be an Argon2id v19 hash (19–256 MiB, 2–10 iterations, 1–8 lanes); use hash-admin-password"
        );
    }
    Ok(())
}

/// Reads a password from stdin, not argv/environment or application logs.
pub fn hash_password() -> Result<()> {
    use std::io::Read;
    let mut input = String::new();
    std::io::stdin().take(4097).read_to_string(&mut input)?;
    let password = input.trim_end_matches(['\r', '\n']);
    if !(12..=1024).contains(&password.len()) {
        bail!("password must contain 12–1024 bytes");
    }
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|_| anyhow::anyhow!("password hashing failed"))?
        .to_string();
    println!("{hash}");
    Ok(())
}

#[derive(Clone)]
struct BrowserSession {
    user: String,
    csrf: String,
    expires: Instant,
}
struct Pending {
    binding: String,
    nonce: Nonce,
    verifier: PkceCodeVerifier,
    metadata: CoreProviderMetadata,
    expires: Instant,
}
#[derive(Default)]
struct Store {
    sessions: HashMap<String, BrowserSession>,
    pending: HashMap<String, Pending>,
    attempts: VecDeque<Instant>,
}
pub struct Auth {
    config: Config,
    service_key: String,
    store: Mutex<Store>,
    password_slots: Arc<tokio::sync::Semaphore>,
    http: reqwest::Client,
}

#[derive(Debug)]
pub struct AuthError(StatusCode, &'static str);
impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"detail": self.1}))).into_response()
    }
}
fn denied() -> AuthError {
    AuthError(StatusCode::UNAUTHORIZED, "Sign-in required.")
}
fn forbidden() -> AuthError {
    AuthError(StatusCode::FORBIDDEN, "Request verification failed.")
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn digest(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn equal(a: &str, b: &str) -> bool {
    Sha256::digest(a.as_bytes())
        .ct_eq(&Sha256::digest(b.as_bytes()))
        .into()
}
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for item in value.to_str().ok()?.split(';') {
            let (key, value) = item.trim().split_once('=')?;
            if key == name {
                if found.is_some() {
                    return None;
                }
                found = Some(value);
            }
        }
    }
    found
}
fn set_cookie(config: &Config, name: &str, value: &str, seconds: u64) -> String {
    format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={seconds}{}",
        if config.secure() { "; Secure" } else { "" }
    )
}
fn append_cookie(response: &mut Response, value: String) {
    response
        .headers_mut()
        .append(header::SET_COOKIE, value.parse().expect("generated cookie"));
}

impl Auth {
    pub fn new(config: Config, service_key: String) -> Result<Arc<Self>> {
        if service_key.is_empty() {
            bail!("CLOUD_LAN_PROXY_ACCESS_CODE is required");
        }
        Ok(Arc::new(Self {
            config,
            service_key,
            store: Mutex::new(Store::default()),
            password_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()?,
        }))
    }
    fn rate_limit(&self) -> Result<(), AuthError> {
        let mut store = self.store.lock().unwrap();
        let now = Instant::now();
        while store
            .attempts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            store.attempts.pop_front();
        }
        if store.attempts.len() >= 10 {
            return Err(AuthError(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many sign-in attempts. Try again in a minute.",
            ));
        }
        store.attempts.push_back(now);
        Ok(())
    }
    fn session(&self, headers: &HeaderMap) -> Option<BrowserSession> {
        let key = digest(cookie(headers, self.config.session_cookie())?);
        let mut store = self.store.lock().unwrap();
        store.sessions.retain(|_, s| s.expires > Instant::now());
        store.sessions.get(&key).cloned()
    }
    fn key_valid(&self, headers: &HeaderMap) -> bool {
        headers
            .get("x-cloud-lan-proxy-key")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| equal(v, &self.service_key))
    }
    fn origin(&self, headers: &HeaderMap) -> Result<(), AuthError> {
        if headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
        {
            return Err(forbidden());
        }
        let origin = headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(forbidden)?;
        let origin_url = Url::parse(origin).map_err(|_| forbidden())?;
        if origin_url.origin().ascii_serialization() != origin {
            return Err(forbidden());
        }
        let expected = if let Some(url) = &self.config.public_url {
            url.origin().ascii_serialization()
        } else {
            // Backward-compatible access-code deployments: never trust forwarded headers.
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(forbidden)?;
            if !matches!(origin_url.scheme(), "http" | "https") {
                return Err(forbidden());
            }
            format!("{}://{host}", origin_url.scheme())
        };
        if origin != expected {
            return Err(forbidden());
        }
        Ok(())
    }
    fn csrf(&self, headers: &HeaderMap, session: &BrowserSession) -> Result<(), AuthError> {
        self.origin(headers)?;
        if !headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| equal(v, &session.csrf))
        {
            return Err(forbidden());
        }
        Ok(())
    }
    fn issue(
        &self,
        headers: &HeaderMap,
        user: String,
        seconds: u64,
    ) -> Result<Response, AuthError> {
        let token = random();
        let csrf = random();
        let mut store = self.store.lock().unwrap();
        store.sessions.retain(|_, s| s.expires > Instant::now());
        if let Some(old) = cookie(headers, self.config.session_cookie()) {
            store.sessions.remove(&digest(old));
        }
        if store.sessions.len() >= 4096 {
            return Err(AuthError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Session capacity reached.",
            ));
        }
        store.sessions.insert(
            digest(&token),
            BrowserSession {
                user: user.clone(),
                csrf: csrf.clone(),
                expires: Instant::now() + Duration::from_secs(seconds),
            },
        );
        let mut response =
            Json(json!({"authenticated": true, "username": user, "csrf_token": csrf}))
                .into_response();
        append_cookie(
            &mut response,
            set_cookie(&self.config, self.config.session_cookie(), &token, seconds),
        );
        Ok(response)
    }
}

pub fn routes(auth: Arc<Auth>) -> Router {
    Router::new()
        .route("/api/auth/session", get(session_status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/oidc/start", post(oidc_start))
        .route("/api/auth/oidc/callback", get(oidc_callback))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
        .with_state(auth)
}

/// Default deny: only an explicit list of public endpoints bypass authentication.
/// Service keys cannot bypass browser authentication when account auth is enabled.
pub async fn guard(State(auth): State<Arc<Auth>>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let method = request.method();
    let public = matches!(
        (method, path),
        (
            &Method::GET | &Method::HEAD,
            "/api/health" | "/api/auth/session" | "/api/auth/oidc/callback" | "/login"
        ) | (
            &Method::POST,
            "/api/auth/login" | "/api/auth/oidc/start" | "/api/auth/logout"
        )
    ) || (matches!(*method, Method::GET | Method::HEAD)
        && path.starts_with("/assets/"));
    let internal = path.starts_with("/internal/");
    let safe = matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS);
    let decision = if public {
        Ok(())
    } else if internal {
        if auth.key_valid(request.headers()) {
            Ok(())
        } else {
            Err(denied())
        }
    } else if auth.config.mode == Mode::AccessCode && auth.key_valid(request.headers()) {
        if !safe && request.headers().contains_key(header::ORIGIN) {
            auth.origin(request.headers())
        } else {
            Ok(())
        }
    } else if let Some(session) = auth.session(request.headers()) {
        if safe {
            Ok(())
        } else {
            auth.csrf(request.headers(), &session)
        }
    } else {
        Err(denied())
    };
    let mut response = match decision {
        Ok(()) => next.run(request).await,
        Err(error) => {
            if error.0 == StatusCode::UNAUTHORIZED
                && !path.starts_with("/api/")
                && !internal
                && safe
            {
                Redirect::to("/login").into_response()
            } else {
                error.into_response()
            }
        }
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::VARY, "Cookie".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
        .headers_mut()
        .insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    response
}

async fn session_status(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Json<Value> {
    let session = auth.session(&headers);
    Json(json!({
        "authenticated": session.is_some(),
        "username": session.as_ref().map(|s| &s.user),
        "csrf_token": session.as_ref().map(|s| &s.csrf),
        "access_code": auth.config.mode == Mode::AccessCode,
        "local": matches!(auth.config.mode, Mode::Local | Mode::Both),
        "oidc": matches!(auth.config.mode, Mode::Oidc | Mode::Both),
    }))
}
#[derive(Deserialize)]
struct Credentials {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    access_code: String,
}
async fn login(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Json(body): Json<Credentials>,
) -> Result<Response, AuthError> {
    auth.origin(&headers)?;
    auth.rate_limit()?;
    let valid = if auth.config.mode == Mode::AccessCode {
        body.access_code.len() <= 1024 && equal(&body.access_code, &auth.service_key)
    } else if matches!(auth.config.mode, Mode::Local | Mode::Both) {
        if body.username.len() > 128 || body.password.len() > 1024 {
            return Err(denied());
        }
        let permit = auth
            .password_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError(StatusCode::TOO_MANY_REQUESTS, "Try again shortly."))?;
        let hash = auth.config.password_hash.clone();
        let name_ok = equal(&body.username, &auth.config.username);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let hash = PasswordHash::new(&hash).expect("validated hash");
            Argon2::default()
                .verify_password(body.password.as_bytes(), &hash)
                .is_ok()
                && name_ok
        })
        .await
        .unwrap_or(false)
    } else {
        false
    };
    if !valid {
        return Err(AuthError(
            StatusCode::UNAUTHORIZED,
            "Credentials were not accepted.",
        ));
    }
    let user = if auth.config.mode == Mode::AccessCode {
        "Administrator".to_owned()
    } else {
        auth.config.username.clone()
    };
    auth.issue(&headers, user, auth.config.session_seconds)
}
async fn logout(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Result<Response, AuthError> {
    auth.origin(&headers)?;
    if let Some(session) = auth.session(&headers) {
        auth.csrf(&headers, &session)?;
    }
    let mut store = auth.store.lock().unwrap();
    if let Some(token) = cookie(&headers, auth.config.session_cookie()) {
        store.sessions.remove(&digest(token));
    }
    if let Some(binding) = cookie(&headers, auth.config.flow_cookie()) {
        store.pending.retain(|_, p| !equal(&p.binding, binding));
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    append_cookie(
        &mut response,
        set_cookie(&auth.config, auth.config.session_cookie(), "", 0),
    );
    append_cookie(
        &mut response,
        set_cookie(&auth.config, auth.config.flow_cookie(), "", 0),
    );
    Ok(response)
}

// The network policy is applied before every discovery/JWKS/token request.
async fn oidc_http(
    client: reqwest::Client,
    allow_loopback_http: bool,
    request: openidconnect::HttpRequest,
) -> Result<openidconnect::HttpResponse, std::io::Error> {
    let url = Url::parse(&request.uri().to_string()).map_err(std::io::Error::other)?;
    if !secure_url(&url) || (url.scheme() != "https" && !allow_loopback_http) {
        return Err(std::io::Error::other("OIDC endpoint must use HTTPS"));
    }
    let response = client
        .request(request.method().clone(), url)
        .headers(request.headers().clone())
        .body(request.body().clone())
        .send()
        .await
        .map_err(std::io::Error::other)?;
    let mut builder = axum::http::Response::builder().status(response.status());
    *builder.headers_mut().unwrap() = response.headers().clone();
    let mut bytes = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(std::io::Error::other)? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(std::io::Error::other("OIDC response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    builder.body(bytes).map_err(std::io::Error::other)
}
fn oidc_failure() -> AuthError {
    AuthError(
        StatusCode::BAD_GATEWAY,
        "Identity provider sign-in failed. Please start again.",
    )
}

async fn oidc_start(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
) -> Result<Response, AuthError> {
    auth.origin(&headers)?;
    auth.rate_limit()?;
    let issuer = auth.config.issuer.as_ref().ok_or_else(denied)?;
    let http_client = auth.http.clone();
    let allow_http = issuer.url().scheme() == "http";
    let http = move |request| oidc_http(http_client.clone(), allow_http, request);
    let metadata = CoreProviderMetadata::discover_async(issuer.clone(), &http)
        .await
        .map_err(|_| oidc_failure())?;
    if !secure_url(metadata.authorization_endpoint().url())
        || (!allow_http && metadata.authorization_endpoint().url().scheme() != "https")
    {
        return Err(oidc_failure());
    }
    let client = CoreClient::from_provider_metadata(
        metadata.clone(),
        ClientId::new(auth.config.client_id.clone()),
        if auth.config.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(auth.config.client_secret.clone()))
        },
    )
    .set_redirect_uri(RedirectUrl::new(auth.config.callback()).map_err(|_| oidc_failure())?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, state, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .set_pkce_challenge(challenge)
        .url();
    let binding = random();
    let mut store = auth.store.lock().unwrap();
    store.pending.retain(|_, p| p.expires > Instant::now());
    if let Some(old) = cookie(&headers, auth.config.flow_cookie()) {
        store.pending.retain(|_, p| !equal(&p.binding, old));
    }
    if store.pending.len() >= 1024 {
        return Err(AuthError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Sign-in capacity reached.",
        ));
    }
    store.pending.insert(
        digest(state.secret()),
        Pending {
            binding: binding.clone(),
            nonce,
            verifier,
            metadata,
            expires: Instant::now() + Duration::from_secs(300),
        },
    );
    let mut response = Json(json!({"url": url.as_str()})).into_response();
    append_cookie(
        &mut response,
        set_cookie(&auth.config, auth.config.flow_cookie(), &binding, 300),
    );
    Ok(response)
}
#[derive(Deserialize)]
struct Callback {
    #[serde(default)]
    state: String,
    #[serde(default)]
    code: String,
}
async fn oidc_callback(
    State(auth): State<Arc<Auth>>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Response {
    match finish_oidc(&auth, &headers, query).await {
        Ok(mut response) => {
            *response.status_mut() = StatusCode::SEE_OTHER;
            response
                .headers_mut()
                .insert(header::LOCATION, "/".parse().unwrap());
            *response.body_mut() = axum::body::Body::empty();
            append_cookie(
                &mut response,
                set_cookie(&auth.config, auth.config.flow_cookie(), "", 0),
            );
            response
        }
        Err(_) => {
            // Never reflect the provider's response, code, state, tokens, or claims.
            let mut response = Redirect::to("/login?auth_error=oidc").into_response();
            append_cookie(
                &mut response,
                set_cookie(&auth.config, auth.config.flow_cookie(), "", 0),
            );
            response
        }
    }
}
async fn finish_oidc(
    auth: &Auth,
    headers: &HeaderMap,
    query: Callback,
) -> Result<Response, AuthError> {
    if auth.config.issuer.is_none() || query.state.len() > 1024 || query.code.len() > 8192 {
        return Err(denied());
    }
    let binding = cookie(headers, auth.config.flow_cookie()).ok_or_else(denied)?;
    let pending = {
        let mut store = auth.store.lock().unwrap();
        let key = digest(&query.state);
        let flow = store.pending.get(&key).ok_or_else(denied)?;
        if flow.expires <= Instant::now() || !equal(&flow.binding, binding) {
            return Err(denied());
        }
        // Single use, even if the provider exchange fails.
        store.pending.remove(&key).unwrap()
    };
    if query.code.is_empty() {
        return Err(denied());
    }
    let client = CoreClient::from_provider_metadata(
        pending.metadata,
        ClientId::new(auth.config.client_id.clone()),
        if auth.config.client_secret.is_empty() {
            None
        } else {
            Some(ClientSecret::new(auth.config.client_secret.clone()))
        },
    )
    .set_redirect_uri(RedirectUrl::new(auth.config.callback()).map_err(|_| oidc_failure())?);
    let http_client = auth.http.clone();
    let allow_http = auth
        .config
        .issuer
        .as_ref()
        .is_some_and(|url| url.url().scheme() == "http");
    let http = move |request| oidc_http(http_client.clone(), allow_http, request);
    let tokens = client
        .exchange_code(AuthorizationCode::new(query.code))
        .map_err(|_| oidc_failure())?
        .set_pkce_verifier(pending.verifier)
        .request_async(&http)
        .await
        .map_err(|_| oidc_failure())?;
    let token = tokens.id_token().ok_or_else(denied)?;
    let verifier = client.id_token_verifier();
    let claims = token
        .claims(&verifier, &pending.nonce)
        .map_err(|_| denied())?;
    // The library deliberately leaves azp policy to the relying party.
    if claims
        .authorized_party()
        .is_some_and(|party| party.as_str() != auth.config.client_id)
        || (claims.audiences().len() > 1 && claims.authorized_party().is_none())
    {
        return Err(denied());
    }
    if let Some(expected) = claims.access_token_hash() {
        let actual = AccessTokenHash::from_token(
            tokens.access_token(),
            token.signing_alg().map_err(|_| denied())?,
            token.signing_key(&verifier).map_err(|_| denied())?,
        )
        .map_err(|_| denied())?;
        if &actual != expected {
            return Err(denied());
        }
    }
    if !auth.config.subjects.contains(claims.subject().as_str()) {
        return Err(denied());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let remaining = (claims.expiration().timestamp().max(0) as u64).saturating_sub(now);
    if remaining == 0 {
        return Err(denied());
    }
    auth.issue(
        headers,
        claims.subject().as_str().to_owned(),
        remaining.min(auth.config.session_seconds),
    )
}
