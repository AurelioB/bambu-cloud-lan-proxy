---
title: HTTP API reference
description: Admin sessions, configuration, cloud sign-in, and internal storage boundaries.
---
The HTTP API runs on the admin origin (default `http://127.0.0.1:4844`). It is **not** Bambu's cloud API and does not replace MQTT/file protocols for printer clients. Paths are unversioned and describe the current implementation; check the source revision when integrating.

Download the [OpenAPI 3.1 reference](/openapi.json). It is a static specification, not a live “try it” interface connected to your printers.

## Authentication

Private admin endpoints accept a browser session cookie. In default `access_code` mode only, direct clients may instead send `X-Cloud-Lan-Proxy-Key: YOUR_PROXY_CODE`. The key cannot bypass `local`, `oidc`, or `both` admin authentication.

Session-authorized mutations require both the correct `Origin` and `X-CSRF-Token`. Obtain the CSRF token from sign-in or `GET /api/auth/session`. Login, logout, and OIDC initiation also verify Origin. There is no bearer-token or personal API-token implementation.

The internal storage interface always requires the explicit service key, in every admin mode. Browser session cookies cannot authorize it. Never expose it as a public file API.

## Public routes

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/health` | `{"status":"ok"}`; liveness only |
| GET | `/api/auth/session` | Current caller's session and available sign-in methods |
| POST | `/api/auth/login` | Exchange code or local credentials for a cookie |
| POST | `/api/auth/logout` | Revoke current session; `204`, Origin and session CSRF checked |
| POST | `/api/auth/oidc/start` | `{ "url": "…" }` and browser-binding cookie |
| GET | `/api/auth/oidc/callback` | Consume code/state and binding cookie; redirect to `/` or `/login?auth_error=oidc` |

Anonymous session response:

```json
{
  "authenticated": false,
  "username": null,
  "csrf_token": null,
  "access_code": true,
  "local": false,
  "oidc": false
}
```

Login accepts `{ "access_code": "…" }` in code mode, or `{ "username": "…", "password": "…" }` when local login is enabled. Success returns `authenticated`, `username`, and `csrf_token`, plus `Set-Cookie`. Treat the cookie and token as secrets. Authentication JSON bodies are limited to 16 KiB.

OIDC is a browser redirect flow, not an API-key generator. Do not POST provider credentials to the proxy or replay callback URLs.

## Read-only API example

Only for **access-code mode**, from a trusted host. Create a mode-600 curl config file outside the repository containing:

```text
header = "X-Cloud-Lan-Proxy-Key: YOUR_PROXY_CODE"
```

Then use its actual path:

```sh
curl --fail-with-body --config /secure/path/proxy-curl.conf \
  http://127.0.0.1:4844/api/admin/overview
```

Do not commit the file, expose its contents in logs, or assume this key works in local/OIDC modes.

## Browser session example

For a custom frontend running on the **same admin origin**, the equivalent request sequence is:

```js
// Credentials come from the user's login form, never hardcoded in source.
const login = await fetch('/api/auth/login', {
  method: 'POST',
  credentials: 'same-origin',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ username, password }),
});
if (!login.ok) throw new Error('Sign-in failed');
const session = await login.json();
const response = await fetch('/api/admin/settings', {
  credentials: 'same-origin',
  cache: 'no-store',
});
if (!response.ok) throw new Error('Session unavailable');
const settings = await response.json();
// Only after an explicit user-confirmed edit:
const saved = await fetch('/api/admin/settings', {
  method: 'PUT',
  credentials: 'same-origin',
  headers: {
    'Content-Type': 'application/json',
    'X-CSRF-Token': session.csrf_token,
  },
  body: JSON.stringify(settings),
});
if (!saved.ok) throw new Error('Settings were not saved');
```

The browser supplies Origin on these same-origin mutations. Do not store credentials or the CSRF token in persistent browser storage. Clear private state on a `401` and discard stale in-flight responses after logout.

## Admin routes

### GET /api/admin/overview

Returns `healthy`, `signed_in`, `cloud_connected`, `region`, and a `printers` array. Each printer includes:

- `serial`, `name`, `model`, `online`, `state`, `secure_control`;
- `virtual_ip`, `physical_ip`, `physical_access_configured`, `endpoint_enabled`;
- `capabilities`: `camera` (`rtsps`, `image`, `unknown`), `local_camera_bridge`, `chamber_temperature`, `dual_nozzle`, `model_known`, `source` (`model_profile`, `runtime_telemetry`, `unknown`), `observed_fields`, and nullable `runtime` evidence;
- `endpoint_health.state`: `disabled`, `healthy`, `degraded`, or `unavailable`;
- `endpoint_health.services`: nullable runtime snapshot with `serial`, `virtual_ip`, `process_running`, `mqtt`, `ftps`, `files`, and nullable `camera`.

`runtime` contains `updated_at` (Unix seconds), nullable `firmware`, `ams_units`, and `filament_slots`, an `ams` object mapping unit IDs to observed slot IDs, observed `sensors`, and a `firmware_controls` object mapping reported function names to booleans. Nullable `dual_nozzle` and `chamber_temperature` record physical evidence. Unknown counts are `null`, while a reported absence is `0`. These observations never grant write permissions. See [features and data paths](/docs/features/) for freshness and fallback rules.

The response never returns the saved physical access code. Health snapshots older than 30 seconds are not reported as healthy. This endpoint is a connection overview, not the complete raw print telemetry stream.

### PUT /api/admin/printers/{serial}/endpoint

```json
{
  "virtual_ip": "192.168.1.201",
  "physical_ip": "192.168.1.42",
  "physical_access_code": "",
  "enabled": true
}
```

The printer must belong to the connected account. An enabled endpoint requires a private/loopback IPv4 address; duplicate addresses are rejected. Physical IP must be private/loopback if supplied. A nonempty physical code must contain eight ASCII letters/digits.

An omitted or empty `physical_access_code` **preserves** the saved code; it is not a delete-credential operation. Success:

```json
{ "ok": true, "certificate_refresh_required": true }
```

The HTTP request saves configuration. It does not regenerate certificates or run the host bootstrap script. Apply the deployment-specific certificate refresh afterward.

### GET /api/admin/settings · PUT /api/admin/settings

GET returns the complete settings object. PUT expects every field and returns the saved object:

```json
{
  "mode": "read_only",
  "allow_cancel": false,
  "cloud_print_enabled": false,
  "remote_reprint_enabled": false,
  "advertise_controls": false,
  "storage_allow_delete": false,
  "storage_allow_upload": false
}
```

Valid UI/persisted modes: `read_only`, `controls`, `full_control`. This is replacement, not a JSON merge patch. Read current settings before changing only selected fields. See [permissions](/docs/permissions/) for independent gates.

### GET /api/admin/ca

Returns the public PEM CA certificate as `application/x-pem-file`, with an attachment filename. `404` if the export is unavailable. This route is authenticated even though the CA itself is safe to distribute.

## Bambu account routes (private)

These are unrelated to **admin OIDC**:

| Method / path | Contract |
| --- | --- |
| GET `/api/cloud/auth/status` | `signed_in`, `cloud_connected`, `region` |
| GET `/api/cloud/auth/url` | `{ "url": "official Bambu sign-in URL" }` |
| POST `/api/cloud/auth/paste` | Body `{ "pasted_url": "callback URL" }`; success `{ "ok": true }` |

The callback string is limited to 20,000 characters. It contains sensitive account material. Never log or include it in examples. Admin sign-out does not sign out this account session.

## Internal storage API

Base: `/internal/cloud-lan-proxy/storage`. Every request uses `X-Cloud-Lan-Proxy-Key`. Cookies alone receive `401`. Keep access restricted to the endpoint workers/private host.

GET `/capabilities` returns `ability`, `list`, `download`, `subfile`, policy-derived `upload`/`delete`, `notifications: "revision"`, and a Unix `time`.

POST `/{operation}` accepts JSON:

| Operation | Request fields | Result / constraints |
| --- | --- | --- |
| `ability` | `serial`, optional `api_version` (default 2) | Plugin capabilities plus `revision: 0` |
| `list` | `serial`, `type`, `storage` | Plugin listing; shape depends on storage/firmware |
| `download` | `serial`, `path` or `name` | `{ "cache_key": "…", "size": 123 }` |
| `subfile` | `serial`, `request` | Same cache result; request is plugin-native JSON |
| `upload` | `serial`, `storage`, `path`, `source_path` | Policy gated; canonical source must be a file inside staging |
| `delete` | `serial`, `paths` (1–100 entries) | Policy gated; firmware-dependent result |
| `discard` | `cache_key` | `{ "discarded": true }`; key is 32 ASCII alphanumeric characters |

All operations other than `discard` require a nonempty serial allowed by the configured printer filter; the underlying plugin also needs a usable printer. Upload/delete results include `revision: 1`; this is not a durable globally increasing revision number.

Downloads do **not** return an HTTP stream or public URL. Workers read the cache key from a shared local cache directory, then discard it. This API assumes matching storage mounts and trusted workers. Long transfers can run up to 21 minutes; list/ability use 60 seconds, delete 90 seconds.

## Errors and retries

Application errors normally return `{ "detail": "message" }`. Framework-generated method/body/JSON rejections may use a different body, so check HTTP status before parsing JSON.

| Status | Typical meaning |
| --- | --- |
| 400 | Invalid endpoint/settings/storage input |
| 401 | Missing, invalid, or expired authentication |
| 403 | Origin/CSRF failure, forbidden printer, disabled storage mutation |
| 404 | Unknown API/operation or unavailable CA |
| 413 / 422 | Oversized request or JSON/body validation failure |
| 429 | Sign-in rate limit |
| 500 / 502 / 503 | Local failure, plugin/upstream failure, or bounded auth capacity |

Do not automatically retry mutating calls after ambiguous network failures. In particular, cloud write timeouts can mean the printer already received the command.
