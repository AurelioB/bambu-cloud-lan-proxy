# Admin authentication

Authentication here protects **only the admin website and its HTTP APIs**. The virtual printers remain compatible with Bambuddy and Bambu Studio.

| Surface | Credential |
| --- | --- |
| Admin UI, printer/account metadata, connection settings, permissions, public CA download, Bambu cloud sign-in endpoints | An authenticated browser session; the legacy admin API key is accepted only in `access_code` mode |
| Virtual-printer MQTT, FTPS, native files, and RTSPS | Existing `PROXY_ACCESS_CODE`, unchanged |
| Internal HTTP storage service used by virtual-printer workers | Existing explicit service-key header, unchanged; browser cookies cannot access it |
| Discovery | Existing LAN discovery behavior, unchanged |

Neither local admin passwords nor OIDC tokens are sent to printers. Admin sign-out does not disconnect printers, invalidate the device code, or sign out the Bambu account.

## Modes

`ADMIN_AUTH_MODE` supports:

- `access_code` (default): optional account authentication is not enabled; entering the existing code exchanges it for a browser session. Direct API clients may still use `X-Cloud-Lan-Proxy-Key`.
- `local`: one built-in administrator username/password. No auth server, database, or extra container.
- `oidc`: single sign-on only. The access code and local password cannot bypass OIDC.
- `both`: OIDC plus the configured local administrator, useful as an intentional recovery option.

There is deliberately **no anonymous-admin mode**. “Optional” refers to enabling account-based sign-in, not making private data public. There is no registration or user-management UI; configure the local administrator on the host and manage SSO users at the identity provider. Every admitted identity is a workspace administrator, not a read-only user.

The feature is bundled in the existing Rust application and works with standalone and Docker Compose deployments. It does not require an authentication sidecar.

## Enable local sign-in

Build the updated application/image before generating a hash.

For a native build, run `cargo run -- hash-admin-password` with the password on stdin. For Compose, this Bash example hides input, avoids command-line password arguments, and does not start dependent printer services:

```bash
read -r -s -p "Admin password: " admin_password
printf '\n'
printf '%s' "$admin_password" | docker compose run --rm --no-deps -T gateway hash-admin-password
unset admin_password
```

Choose at least 12 characters. Copy the resulting Argon2id PHC hash into `.env`, enclosed in **single quotes** so Compose preserves its dollar signs:

```dotenv
ADMIN_AUTH_MODE=local
ADMIN_PUBLIC_URL=https://printers.example.com
ADMIN_USERNAME=admin
ADMIN_PASSWORD_HASH='<paste the complete generated Argon2id hash here>'
ADMIN_SESSION_SECONDS=3600
```

Restart the shared core (`gateway` is its historical Compose service name) when ready to apply the configuration. Changing authentication configuration requires a restart; it does not change the virtual-printer access code.

`ADMIN_PUBLIC_URL` is the exact external origin, including a nonstandard port if used. It must be HTTPS outside loopback, with no path, query, fragment, or user credentials. HTTP loopback origins such as `http://127.0.0.1:4844` are supported for development. Host-only secure session cookies follow the configured origin. The application does not trust forwarded headers to determine this security policy.

Terminate HTTPS at a trusted reverse proxy and proxy requests without stripping Origin, Cookie, or CSRF headers. Do not configure wildcard CORS or cache authenticated responses. Subpath hosting is not supported. Keep the raw admin port bound to loopback or an isolated private network.

## Enable OIDC

Register a web client at an OIDC provider. Configure this exact redirect URI:

```text
https://printers.example.com/api/auth/oidc/callback
```

Use Authorization Code flow, PKCE S256, and the `openid` scope. The client supports confidential clients using a client secret and public clients without one. With a client secret, token endpoint authentication uses HTTP Basic.

```dotenv
ADMIN_AUTH_MODE=oidc
ADMIN_PUBLIC_URL=https://printers.example.com
ADMIN_OIDC_ISSUER=https://identity.example.com/your-issuer
ADMIN_OIDC_CLIENT_ID=bambu-admin
ADMIN_OIDC_CLIENT_SECRET='<your client secret>'
ADMIN_OIDC_ALLOWED_SUBJECTS=exact-subject-id-1,exact-subject-id-2
ADMIN_SESSION_SECONDS=3600
```

Use the issuer **exactly** as advertised by your provider, including any trailing slash. Discovery and signing keys are fetched when starting sign-in. The provider must support discovery, JWKS, and query-mode authorization-code callbacks. Refresh tokens, implicit flow, and form-post callbacks are not used.

Allowed subjects are exact, case-sensitive `sub` claim values for this issuer/client, not email addresses, display names, or email domains. Obtain them from the provider's user/client administration. Empty allowlists fail startup rather than granting the entire organization access. Client-specific/pairwise subjects must match those issued to this client.

Use `ADMIN_AUTH_MODE=both` and the local settings above to offer both choices. This is an explicit password fallback; do not enable it if your security policy requires all administrators to pass the provider's MFA policy. Enforce MFA at the identity provider.

Provider failures fail closed. Local login in `both` mode remains available if discovery fails. Provider metadata is refreshed at each new login so subsequent logins pick up signing-key rotations.

## Secret files

Native deployments can use `ADMIN_PASSWORD_HASH_FILE` and `ADMIN_OIDC_CLIENT_SECRET_FILE` instead of inline values. For Compose, these variables are passed through; mount the corresponding files into the shared core. For example, add a private `compose.override.yaml`:

```yaml
services:
  gateway:
    environment:
      ADMIN_PASSWORD_HASH: ""
      ADMIN_PASSWORD_HASH_FILE: /run/secrets/admin-password-hash
      ADMIN_OIDC_CLIENT_SECRET: ""
      ADMIN_OIDC_CLIENT_SECRET_FILE: /run/secrets/oidc-client-secret
    secrets:
      - admin-password-hash
      - oidc-client-secret

secrets:
  admin-password-hash:
    file: ./.secrets/admin-password-hash
  oidc-client-secret:
    file: ./.secrets/oidc-client-secret
```

Include only the secrets needed for your mode. Do not set both a value and its file variant. Keep `.secrets/`, local overrides, and `.env` out of version control and readable only by the operator/runtime as appropriate.

## Enforcement

A single default-deny middleware wraps the entire HTTP router, including fallback and future routes. Private HTML requests redirect to `/login`; unauthenticated API requests return 401 without invoking private handlers. The previously public `/api/cloud/auth/status` now requires authentication. Unknown API routes return 404 after authentication, never the SPA HTML.

Only the sign-in shell, its static assets, minimal `/api/health`, and the explicit browser-auth endpoints are public. `/api/auth/session` exposes configured sign-in choices and the caller's own session, never printer or Bambu-account data. Health checks use only the non-sensitive health endpoint.

Browser controls include:

- High-entropy opaque session IDs stored only in HttpOnly, SameSite=Lax, host-only cookies; Secure and `__Host-` cookies for HTTPS.
- In-memory server-side sessions indexed by a SHA-256 digest. No passwords, device codes, ID tokens, or refresh tokens in browser storage.
- Absolute session lifetime, default one hour (configurable 300–86400 seconds); OIDC sessions cannot outlive the verified ID token.
- New session on sign-in, replacement-session invalidation, revocation on sign-out, and invalidation of all sessions on server restart.
- Origin verification for sign-in, sign-out, and OIDC initiation. Session-authorized changes additionally require a per-session CSRF token.
- Bounded session/login-flow stores, 10 sign-in attempts/starts per minute globally, a two-verification password concurrency limit, and limited request/identity-provider response sizes. The global limiter is intentionally conservative for a small self-hosted console; a trusted reverse proxy can add network-level throttling.
- Argon2id password verification and generic credential failures.
- OIDC signature, issuer, audience, authorized-party (when present), expiration, nonce, and access-token-hash checks. PKCE, single-use state, and an independent browser-binding cookie protect callbacks. Pending flows expire after five minutes.
- OIDC HTTP redirects disabled, HTTPS required outside loopback, bounded network timeouts, and no reflection of provider codes/tokens/errors into the browser.
- No-store responses, no-referrer policy, frame denial, and nosniff headers.

The UI checks the session before fetching private data, removes old access codes left in session storage, clears private views on 401, and ignores responses from requests predating a sign-out. Sign-out is broadcast to other open tabs. If server-side sign-out fails, the UI hides private data and explicitly asks for a retry rather than claiming the session was revoked.

Sign-out is local to this application. It does not terminate the provider's SSO session. Removing a user at the provider does not immediately revoke an already-issued application session; it expires at the configured/token deadline, or restart the core to invalidate all sessions immediately. Sessions are intentionally not shared across multiple core replicas. In-flight requests already authorized before sign-out cannot be undone.

Keep virtual-printer ports on a trusted LAN/VPN. Web authentication does not secure LAN discovery, strengthen the device-code protocol, or make printer ports safe to publish on the internet.

## Verification

```sh
cargo test --locked
cd web
npm run check
npm run build
npm run test:ui
```

Rust tests exercise the real middleware and local password verification. A loopback OIDC test provider signs real EdDSA ID tokens, verifies PKCE at exchange, and tests correct sign-in plus wrong signature/issuer/audience/authorized party/nonce, expiration, allowlist rejection, access-token substitution, browser-binding failure, expired state, and callback replay. No production credentials or printer connection are needed.

Browser tests cover all private deep links before sign-in, local/OIDC/both presentations, local sign-in and reload, CSRF headers, secret-storage removal, session expiration during a save, late-response isolation after sign-out, rejected callbacks, failed-session checks, and retrying failed sign-out. Provider-specific production setup still requires your own issuer/client configuration and verification; no real external identity provider was configured during implementation.

Implementation references: [openidconnect](https://docs.rs/openidconnect/latest/openidconnect/), [RustCrypto Argon2](https://docs.rs/argon2/latest/argon2/), [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html), [OWASP CSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).
