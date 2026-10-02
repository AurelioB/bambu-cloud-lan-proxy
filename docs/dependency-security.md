# Dependency security review

Reviewed for the v0.7.0 candidate on 2026-10-02. Exceptions are not declarations
that the affected upstream libraries are fixed. Review this document and
`.cargo/audit.toml` whenever dependency versions or cryptographic usage change.

## RSA / RUSTSEC-2023-0071

`openidconnect 4.0.1` depends on `rsa 0.9.10`. The [Marvin advisory](https://rustsec.org/advisories/RUSTSEC-2023-0071)
describes private-key timing leakage; no fixed stable upgrade is available.
The proxy is an OIDC relying party: it verifies providers' ID-token signatures
using public JWKs. It does not decrypt with RSA, issue RSA-signed tokens, load
an RSA signing key, or use `private_key_jwt` client authentication. The bundled
administrator uses Argon2, not RSA. The loopback test provider uses Ed25519.

The exception depends on that verification-only boundary. Adding RSA signing,
decryption, encrypted ID tokens, or client-assertion authentication requires a
new review before release. OIDC signature verification remains enabled.

## Unused webpki 0.102 / four 2026 advisories

`rumqttc 0.25.1` includes `rustls-webpki 0.102.8` with its Rustls feature.
In that exact release, its only `webpki::` reference is the `Error::WebPki`
enum variant in `src/tls.rs`; it does not call that library's certificate,
name-constraint, or CRL verification functions. Actual TLS uses Rustls 0.23
and its patched `rustls-webpki 0.103.13` dependency.

Our local MQTT path supplies a custom Rustls configuration with a pinned
physical-printer certificate. The internal MQTT connection is plaintext on a
private broker; it is not an internet-facing service. OIDC/cloud HTTPS uses
Reqwest/Rustls, not this old webpki dependency.

Exceptions cover [0049](https://rustsec.org/advisories/RUSTSEC-2026-0049),
[0098](https://rustsec.org/advisories/RUSTSEC-2026-0098),
[0099](https://rustsec.org/advisories/RUSTSEC-2026-0099), and
[0104](https://rustsec.org/advisories/RUSTSEC-2026-0104).
The affected code is not used by these paths. Remove the exceptions when an
upstream MQTT release removes or updates the unused dependency. No Gateway
dependency, forked TLS implementation, or certificate-verification bypass was
introduced to make the audit pass.

## Informational warning

`rustls-pemfile 2.2.0` is archived and wraps the maintained parsing code in
`rustls-pki-types`. It is also used by the current MQTT dependency. The audit
retains its unmaintained warning; it is not a known vulnerability suppression.
Moving our own PEM calls is a small optional maintenance change, not unfinished
Rust migration. The yanked `yoke-derive 0.8.3` was updated to 0.8.4.

## Reproduce

```sh
auditor=$(./scripts/audit-tool.sh cargo-audit)
"$auditor" audit --file Cargo.lock
```

CI audits the lockfile and fails on newly reported vulnerabilities. This is
dependency screening, not a complete application security audit. The proprietary
plugin is downloaded separately and is not covered by Rust/npm advisory scans.
