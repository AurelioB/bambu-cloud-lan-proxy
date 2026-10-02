#!/bin/sh
# Run ONLY inside the disposable bootstrap image with --network none.
set -eu
test -f /etc/alpine-release
export PROXY_ACCESS_CODE=TEST1234 PROXY_BIND_IP=127.0.0.2
export PROXY_RUNTIME_UID=1000 PROXY_RUNTIME_GID=1000 PROXY_RUNTIME_CERT_GID=1000
export PROXY_STAGING_DIR=/test-state/staging
export PROXY_CAMERA_STATE_DIR=/test-state/camera
export PROXY_REMOTE_CACHE_DIR=/test-state/cache
export PROXY_LOCAL_STATUS_DIR=/test-state/status
export PROXY_ENDPOINT_STATE_DIR=/test-state/endpoints
export PROXY_ADDRESS_STATE_DIR=/test-state/addresses
export PROXY_PRINTER_REGISTRY=/test-state/printers.json
mkdir -p /test-state
printf '%s\n' '[{"serial":"DEMO00001","virtual_ip":"192.168.1.201","endpoint_enabled":true}]' > "$PROXY_PRINTER_REGISTRY"

# Default container layout still works.
/usr/local/bin/bootstrap
test -s /data/export/bambu-cloud-lan-proxy-ca.crt

# Standalone path overrides use the same implementation.
export PROXY_BOOTSTRAP_DIR=/test-state/tls
mkdir -p "$PROXY_BOOTSTRAP_DIR"
# Native installation pre-creates this parent with service ownership; otherwise
# root's umask would prevent runtime services from traversing it.
chown 1000:1000 "$PROXY_BOOTSTRAP_DIR"
chmod 750 "$PROXY_BOOTSTRAP_DIR"
/usr/local/bin/bootstrap
test "$(stat -c %u:%g /test-state/tls)" = 1000:1000
openssl verify -CAfile /test-state/tls/certs/ca.crt /test-state/tls/certs/server.crt
openssl x509 -in /test-state/tls/certs/server.crt -noout -checkip 192.168.1.201
test "$(stat -c %a /test-state/tls/certs/ca.key)" = 600
test "$(stat -c %u /test-state/tls/certs/ca.key)" = 0
test "$(stat -c %u:%g /test-state/tls/runtime/certs/server.key)" = 1000:1000
adduser -D -H -u 1000 -s /bin/sh bootstrap-test
su -s /bin/sh bootstrap-test -c 'test -r /test-state/tls/runtime/certs/server.key && test -r /test-state/tls/runtime/auth/passwords && ! test -r /test-state/tls/certs/ca.key'
ca_before=$(sha256sum /test-state/tls/certs/ca.crt)
server_before=$(sha256sum /test-state/tls/certs/server.crt)
/usr/local/bin/bootstrap
test "$ca_before" = "$(sha256sum /test-state/tls/certs/ca.crt)"
test "$server_before" = "$(sha256sum /test-state/tls/certs/server.crt)"
printf '%s\n' '[{"serial":"DEMO00001","virtual_ip":"192.168.1.202","endpoint_enabled":true}]' > "$PROXY_PRINTER_REGISTRY"
/usr/local/bin/bootstrap
test "$ca_before" = "$(sha256sum /test-state/tls/certs/ca.crt)"
test "$server_before" != "$(sha256sum /test-state/tls/certs/server.crt)"
openssl x509 -in /test-state/tls/certs/server.crt -noout -checkip 192.168.1.202
echo 'Container and standalone bootstrap: certificate trust, permissions, idempotency, and SAN refresh passed.'
