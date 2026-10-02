use super::*;

#[tokio::test]
async fn legacy_access_code_can_create_a_session_but_cannot_enter_oidc_only_mode() {
    let router = app(auth(Mode::AccessCode));
    let response = router
        .clone()
        .oneshot(req(
            "POST",
            "/api/auth/login",
            &[("origin", "https://console.example.test")],
            json!({"access_code":"LANKEY12"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie(&response);
    assert_eq!(
        router
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response=app(auth(Mode::Oidc)).oneshot(req("POST", "/api/auth/login", &[("origin","https://console.example.test")], json!({"access_code":"LANKEY12","username":"admin","password":"correct horse battery staple"}))).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

use axum::{
    body::{Body, to_bytes},
    http::Request as HttpRequest,
    routing::any,
};
use openidconnect::{
    AccessToken, JsonWebKeyId, PrivateSigningKey,
    core::{CoreEdDsaPrivateSigningKey, CoreIdToken, CoreIdTokenClaims, CoreJwsSigningAlgorithm},
};
use tower::ServiceExt;

fn config(mode: Mode) -> Config {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    Config {
        mode,
        public_url: Some(Url::parse("https://console.example.test").unwrap()),
        username: "admin".into(),
        password_hash: HASH
            .get_or_init(|| {
                Argon2::default()
                    .hash_password(
                        b"correct horse battery staple",
                        &SaltString::generate(&mut OsRng),
                    )
                    .unwrap()
                    .to_string()
            })
            .clone(),
        issuer: None,
        client_id: "console-client".into(),
        client_secret: String::new(),
        subjects: HashSet::from(["allowed-user".into()]),
        session_seconds: 3600,
    }
}
fn auth(mode: Mode) -> Arc<Auth> {
    Auth::new(config(mode), "LANKEY12".into()).unwrap()
}
fn app(auth: Arc<Auth>) -> Router {
    routes(auth.clone())
        .route(
            "/api/health",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/login", get(|| async { "public shell" }))
        .route("/assets/test.js", get(|| async { "public static asset" }))
        .fallback(any(|| async { "PRIVATE HANDLER" }))
        .layer(axum::middleware::from_fn_with_state(auth, guard))
}
fn req(method: &str, path: &str, headers: &[(&str, &str)], body: Value) -> HttpRequest<Body> {
    let mut builder = HttpRequest::builder()
        .method(method)
        .uri(path)
        .header("host", "console.example.test");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder
        .header("content-type", "application/json")
        .body(if body.is_null() {
            Body::empty()
        } else {
            Body::from(body.to_string())
        })
        .unwrap()
}
async fn value(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1_000_000).await.unwrap()).unwrap()
}
fn session_cookie(response: &Response) -> String {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|v| {
            let s = v.to_str().unwrap();
            s.starts_with("__Host-bambu-session=")
                .then(|| s.split(';').next().unwrap().to_owned())
        })
        .unwrap()
}
async fn sign_in(router: &Router) -> (String, String) {
    let response = router
        .clone()
        .oneshot(req(
            "POST",
            "/api/auth/login",
            &[("origin", "https://console.example.test")],
            json!({"username":"admin","password":"correct horse battery staple"}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie(&response);
    let csrf = value(response).await["csrf_token"]
        .as_str()
        .unwrap()
        .to_owned();
    (cookie, csrf)
}

#[tokio::test]
async fn private_routes_are_denied_by_default_in_every_mode() {
    for mode in [Mode::AccessCode, Mode::Local, Mode::Oidc, Mode::Both] {
        let router = app(auth(mode));
        for path in [
            "/api/admin/overview",
            "/api/admin/settings",
            "/api/admin/ca",
            "/api/admin/printers/demo/endpoint",
            "/api/cloud/auth/status",
            "/api/cloud/auth/url",
            "/api/cloud/auth/paste",
            "/internal/cloud-lan-proxy/storage/capabilities",
            "/internal/cloud-lan-proxy/storage/download",
            "/api/future-private-route",
        ] {
            for method in ["GET", "HEAD", "POST", "PUT", "DELETE"] {
                let response = router
                    .clone()
                    .oneshot(req(method, path, &[], Value::Null))
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    StatusCode::UNAUTHORIZED,
                    "{method} {path}"
                );
                assert_eq!(response.headers()["cache-control"], "no-store");
            }
        }
        for path in ["/", "/?tab=account", "/?tab=settings", "/connections"] {
            let response = router
                .clone()
                .oneshot(req("GET", path, &[], Value::Null))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()["location"], "/login");
        }
        for path in [
            "/login",
            "/assets/test.js",
            "/api/health",
            "/api/auth/session",
        ] {
            assert_eq!(
                router
                    .clone()
                    .oneshot(req("GET", path, &[], Value::Null))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        let status = value(
            router
                .oneshot(req("GET", "/api/auth/session", &[], Value::Null))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(status["authenticated"], false);
        assert!(status["username"].is_null() && status["csrf_token"].is_null());
        assert!(status.get("region").is_none() && status.get("printers").is_none());
    }
}

#[test]
fn configuration_fails_closed() {
    let cfg = config(Mode::Local);
    let parse = |mode, origin, hash: String, issuer: String, subjects: String| {
        Config::parse(
            mode,
            origin,
            "admin".into(),
            hash,
            issuer,
            "client".into(),
            "".into(),
            subjects,
            3600,
        )
    };
    assert!(parse("disabled", "", "".into(), "".into(), "".into()).is_err());
    assert!(parse("local", "", cfg.password_hash.clone(), "".into(), "".into()).is_err());
    assert!(
        parse(
            "local",
            "http://printer.example.test",
            cfg.password_hash.clone(),
            "".into(),
            "".into()
        )
        .is_err()
    );
    assert!(
        parse(
            "local",
            "https://console.example.test/path",
            cfg.password_hash.clone(),
            "".into(),
            "".into()
        )
        .is_err()
    );
    assert!(
        parse(
            "local",
            "https://console.example.test",
            "plaintext".into(),
            "".into(),
            "".into()
        )
        .is_err()
    );
    assert!(
        parse(
            "oidc",
            "https://console.example.test",
            "".into(),
            "https://id.example.test".into(),
            "".into()
        )
        .is_err()
    );
    assert!(
        parse(
            "local",
            "http://127.0.0.1:4844",
            cfg.password_hash,
            "".into(),
            "".into()
        )
        .is_ok()
    );
}

#[tokio::test]
async fn service_key_cannot_bypass_account_auth_and_cookies_cannot_access_internal_services() {
    for mode in [Mode::Local, Mode::Oidc, Mode::Both] {
        let a = auth(mode);
        let router = app(a.clone());
        for path in [
            "/api/admin/overview",
            "/api/cloud/auth/status",
            "/api/admin/settings",
        ] {
            assert_eq!(
                router
                    .clone()
                    .oneshot(req(
                        "GET",
                        path,
                        &[("x-cloud-lan-proxy-key", "LANKEY12")],
                        Value::Null
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            router
                .clone()
                .oneshot(req(
                    "GET",
                    "/internal/cloud-lan-proxy/storage/capabilities",
                    &[("x-cloud-lan-proxy-key", "LANKEY12")],
                    Value::Null
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            router
                .clone()
                .oneshot(req(
                    "POST",
                    "/internal/cloud-lan-proxy/storage/list",
                    &[("x-cloud-lan-proxy-key", "LANKEY12")],
                    json!({"serial":"test-printer"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let issued = a.issue(&HeaderMap::new(), "admin".into(), 300).unwrap();
        let cookie = session_cookie(&issued);
        assert_eq!(
            router
                .oneshot(req(
                    "GET",
                    "/internal/cloud-lan-proxy/storage/capabilities",
                    &[("cookie", &cookie)],
                    Value::Null
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        app(auth(Mode::AccessCode))
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("x-cloud-lan-proxy-key", "LANKEY12")],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn local_login_csrf_rotation_expiry_and_logout() {
    let a = auth(Mode::Local);
    let router = app(a.clone());
    let (cookie, csrf) = sign_in(&router).await;
    assert_eq!(
        router
            .clone()
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    for headers in [
        vec![("cookie", cookie.as_str())],
        vec![
            ("cookie", cookie.as_str()),
            ("origin", "https://evil.example"),
            ("x-csrf-token", csrf.as_str()),
        ],
        vec![
            ("cookie", cookie.as_str()),
            ("origin", "https://console.example.test"),
            ("x-csrf-token", "wrong"),
        ],
        vec![
            ("cookie", cookie.as_str()),
            ("origin", "null"),
            ("x-csrf-token", csrf.as_str()),
        ],
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(req("PUT", "/api/admin/settings", &headers, json!({})))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let headers = [
        ("cookie", cookie.as_str()),
        ("origin", "https://console.example.test"),
        ("x-csrf-token", csrf.as_str()),
    ];
    assert_eq!(
        router
            .clone()
            .oneshot(req("PUT", "/api/admin/settings", &headers, json!({})))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = router
        .clone()
        .oneshot(req("POST", "/api/auth/logout", &headers, Value::Null))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        router
            .clone()
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let (old_cookie, _) = sign_in(&router).await;
    let response = router
        .clone()
        .oneshot(req(
            "POST",
            "/api/auth/login",
            &[
                ("origin", "https://console.example.test"),
                ("cookie", &old_cookie),
            ],
            json!({"username":"admin","password":"correct horse battery staple"}),
        ))
        .await
        .unwrap();
    let raw_cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(
        raw_cookie.contains("HttpOnly")
            && raw_cookie.contains("SameSite=Lax")
            && raw_cookie.contains("Secure")
            && !raw_cookie.contains("Domain=")
    );
    let new_cookie = session_cookie(&response);
    assert_ne!(old_cookie, new_cookie);
    assert_eq!(
        router
            .clone()
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &old_cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for s in a.store.lock().unwrap().sessions.values_mut() {
        s.expires = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        router
            .clone()
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &new_cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app(auth(Mode::Local))
            .oneshot(req(
                "GET",
                "/api/admin/overview",
                &[("cookie", &new_cookie)],
                Value::Null
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_rejects_cross_origin_and_throttles_guesses() {
    let router = app(auth(Mode::Local));
    for origin in [
        "https://evil.example",
        "null",
        "https://console.example.test.evil.example",
    ] {
        assert_eq!(
            router
                .clone()
                .oneshot(req(
                    "POST",
                    "/api/auth/login",
                    &[("origin", origin)],
                    json!({"username":"admin","password":"correct horse battery staple"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    for _ in 0..10 {
        assert_eq!(
            router
                .clone()
                .oneshot(req(
                    "POST",
                    "/api/auth/login",
                    &[("origin", "https://console.example.test")],
                    json!({"username":"admin","password":"wrong"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        router
            .oneshot(req(
                "POST",
                "/api/auth/login",
                &[("origin", "https://console.example.test")],
                json!({})
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

// Deliberately public test-only key. Never used by the application.
const TEST_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIAYdrYUdG6UqUk80YvIP+afHPEl/Yl503o/d+imf5BS/\n-----END PRIVATE KEY-----";
fn signing_key() -> CoreEdDsaPrivateSigningKey {
    CoreEdDsaPrivateSigningKey::from_ed25519_pem(
        TEST_KEY,
        Some(JsonWebKeyId::new("test-key".into())),
    )
    .unwrap()
}
#[derive(Clone)]
struct Provider {
    issuer: String,
    nonce: Arc<Mutex<String>>,
    challenge: Arc<Mutex<String>>,
    fault: Arc<Mutex<String>>,
    exchanges: Arc<std::sync::atomic::AtomicUsize>,
}
async fn provider_token(
    State(provider): State<Provider>,
    axum::Form(body): axum::Form<HashMap<String, String>>,
) -> Json<Value> {
    provider
        .exchanges
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(body.get("code").unwrap(), "test-code");
    assert_eq!(body.get("grant_type").unwrap(), "authorization_code");
    assert_eq!(
        body.get("redirect_uri").unwrap(),
        "https://console.example.test/api/auth/oidc/callback"
    );
    assert_eq!(body.get("client_id").unwrap(), "console-client");
    let verifier = PkceCodeVerifier::new(body.get("code_verifier").unwrap().clone());
    assert_eq!(
        PkceCodeChallenge::from_code_verifier_sha256(&verifier).as_str(),
        provider.challenge.lock().unwrap().as_str()
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fault = provider.fault.lock().unwrap().clone();
    let claims: CoreIdTokenClaims = serde_json::from_value(json!({
        "iss": if fault == "issuer" { "https://evil.example" } else { &provider.issuer },
        "aud": if fault == "audience" { "other-client" } else { "console-client" },
        "azp": if fault == "authorized_party" { "other-client" } else { "console-client" },
        "exp": if fault == "expired" { now-60 } else { now+300 },
        "iat": now,
        "sub": if fault == "subject" { "not-allowed" } else { "allowed-user" },
        "nonce": if fault == "nonce" { "wrong".to_owned() } else { provider.nonce.lock().unwrap().clone() },
    })).unwrap();
    let token = CoreIdToken::new(
        claims,
        &signing_key(),
        CoreJwsSigningAlgorithm::EdDsa,
        Some(&AccessToken::new("test-access".into())),
        None,
    )
    .unwrap();
    let mut token = token.to_string();
    if fault == "signature" {
        let i = token.rfind('.').unwrap() + 1;
        token.replace_range(i..i + 1, if &token[i..i + 1] == "A" { "B" } else { "A" });
    }
    Json(
        json!({"access_token": if fault == "access_hash" { "substituted-access" } else { "test-access" }, "token_type":"Bearer","id_token":token}),
    )
}
async fn provider() -> (Provider, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let p = Provider {
        issuer: format!("http://{}", listener.local_addr().unwrap()),
        nonce: Default::default(),
        challenge: Default::default(),
        fault: Default::default(),
        exchanges: Default::default(),
    };
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(|State(p): State<Provider>| async move { Json(json!({
            "issuer":p.issuer,"authorization_endpoint":format!("{}/authorize",p.issuer),"token_endpoint":format!("{}/token",p.issuer),
            "jwks_uri":format!("{}/jwks",p.issuer),"response_types_supported":["code"],"subject_types_supported":["public"],
            "id_token_signing_alg_values_supported":["EdDSA"],"code_challenge_methods_supported":["S256"]
        })) }))
        .route("/jwks", get(|| async { Json(json!({"keys":[signing_key().as_verification_key()]})) }))
        .route("/token", post(provider_token)).with_state(p.clone());
    (
        p,
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }),
    )
}
async fn begin_oidc(router: &Router, provider: &Provider) -> (String, String) {
    let response = router
        .clone()
        .oneshot(req(
            "POST",
            "/api/auth/oidc/start",
            &[("origin", "https://console.example.test")],
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let binding = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body = value(response).await;
    let url = Url::parse(body["url"].as_str().unwrap()).unwrap();
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(params["response_type"], "code");
    assert_eq!(params["code_challenge_method"], "S256");
    assert!(params["scope"].split(' ').any(|s| s == "openid"));
    *provider.nonce.lock().unwrap() = params["nonce"].clone();
    *provider.challenge.lock().unwrap() = params["code_challenge"].clone();
    (binding, params["state"].clone())
}

#[tokio::test]
async fn oidc_real_signed_tokens_enforce_claims_signature_allowlist_and_pkce() {
    let (p, server) = provider().await;
    for fault in [
        "",
        "issuer",
        "audience",
        "authorized_party",
        "expired",
        "subject",
        "nonce",
        "signature",
        "access_hash",
    ] {
        let mut cfg = config(Mode::Oidc);
        cfg.issuer = Some(IssuerUrl::new(p.issuer.clone()).unwrap());
        let router = app(Auth::new(cfg, "LANKEY12".into()).unwrap());
        *p.fault.lock().unwrap() = fault.into();
        let (binding, state) = begin_oidc(&router, &p).await;
        let response = router
            .clone()
            .oneshot(req(
                "GET",
                &format!("/api/auth/oidc/callback?state={state}&code=test-code"),
                &[("cookie", &binding)],
                Value::Null,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        if fault.is_empty() {
            assert_eq!(response.headers()["location"], "/");
            let cookie = session_cookie(&response);
            assert_eq!(
                router
                    .oneshot(req(
                        "GET",
                        "/api/admin/overview",
                        &[("cookie", &cookie)],
                        Value::Null
                    ))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        } else {
            assert_eq!(
                response.headers()["location"],
                "/login?auth_error=oidc",
                "{fault}"
            );
            assert!(
                response
                    .headers()
                    .get_all(header::SET_COOKIE)
                    .iter()
                    .all(|v| !v.to_str().unwrap().starts_with("__Host-bambu-session="))
            );
        }
    }
    server.abort();
}

#[tokio::test]
async fn oidc_state_is_browser_bound_expiring_and_single_use() {
    let (p, server) = provider().await;
    let mut cfg = config(Mode::Oidc);
    cfg.issuer = Some(IssuerUrl::new(p.issuer.clone()).unwrap());
    let a = Auth::new(cfg, "LANKEY12".into()).unwrap();
    let router = app(a.clone());
    let (binding, state) = begin_oidc(&router, &p).await;
    let url = format!("/api/auth/oidc/callback?state={state}&code=test-code");
    for wrong in ["", "__Host-bambu-login=attacker"] {
        let response = router
            .clone()
            .oneshot(req("GET", &url, &[("cookie", wrong)], Value::Null))
            .await
            .unwrap();
        assert_eq!(response.headers()["location"], "/login?auth_error=oidc");
    }
    assert_eq!(p.exchanges.load(std::sync::atomic::Ordering::Relaxed), 0);
    let response = router
        .clone()
        .oneshot(req("GET", &url, &[("cookie", &binding)], Value::Null))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/");
    let response = router
        .clone()
        .oneshot(req("GET", &url, &[("cookie", &binding)], Value::Null))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/login?auth_error=oidc");
    assert_eq!(p.exchanges.load(std::sync::atomic::Ordering::Relaxed), 1);
    let (binding, state) = begin_oidc(&router, &p).await;
    for flow in a.store.lock().unwrap().pending.values_mut() {
        flow.expires = Instant::now() - Duration::from_secs(1);
    }
    let response = router
        .oneshot(req(
            "GET",
            &format!("/api/auth/oidc/callback?state={state}&code=test-code"),
            &[("cookie", &binding)],
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/login?auth_error=oidc");
    assert_eq!(p.exchanges.load(std::sync::atomic::Ordering::Relaxed), 1);
    server.abort();
}
