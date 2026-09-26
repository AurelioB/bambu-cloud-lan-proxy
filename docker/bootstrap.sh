#!/bin/sh
set -eu

umask 077

case "${PROXY_ACCESS_CODE:-}" in
  [A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]) ;;
  *)
    echo "PROXY_ACCESS_CODE must contain exactly eight ASCII letters or digits" >&2
    exit 2
    ;;
esac

bind_ip="${PROXY_BIND_IP:-127.0.0.2}"
case "$bind_ip" in
  *[!0-9.]*|.*|*.)
    echo "PROXY_BIND_IP must be an IPv4 address" >&2
    exit 2
    ;;
esac

runtime_uid="${PROXY_RUNTIME_UID:-1883}"
runtime_gid="${PROXY_RUNTIME_GID:-1883}"
case "$runtime_uid" in
  ''|*[!0-9]*) echo "PROXY_RUNTIME_UID must be numeric" >&2; exit 2 ;;
esac
case "$runtime_gid" in
  ''|*[!0-9]*) echo "PROXY_RUNTIME_GID must be numeric" >&2; exit 2 ;;
esac

cert_dir=/data/certs
auth_dir=/data/auth
export_dir=/data/export
runtime_dir=/data/runtime
runtime_cert_dir=$runtime_dir/certs
runtime_auth_dir=$runtime_dir/auth
camera_state_dir=${PROXY_CAMERA_STATE_DIR:-/camera-state}
remote_cache_dir=${PROXY_REMOTE_CACHE_DIR:-/remote-cache}
local_status_dir=${PROXY_LOCAL_STATUS_DIR:-/local-status}
mkdir -p \
  "$cert_dir" \
  "$auth_dir" \
  "$export_dir" \
  "$runtime_cert_dir" \
  "$runtime_auth_dir" \
  "$camera_state_dir" \
  "$remote_cache_dir"
mkdir -p "$local_status_dir"

staging_dir=${PROXY_STAGING_DIR:-/staging}
mkdir -p \
  "$staging_dir/cache" \
  "$staging_dir/timelapse" \
  "$staging_dir/video" \
  "$staging_dir/model" \
  "$staging_dir/data/Metadata"

if [ ! -s "$cert_dir/ca.key" ] || [ ! -s "$cert_dir/ca.crt" ]; then
  echo "Generating local proxy certificate authority"
  openssl genrsa -out "$cert_dir/ca.key" 3072 >/dev/null 2>&1
  openssl req -x509 -new -sha256 -days 3650 \
    -key "$cert_dir/ca.key" \
    -out "$cert_dir/ca.crt" \
    -subj "/CN=Bambu Cloud LAN Proxy Local CA/O=Bambu Cloud LAN Proxy" \
    -addext "basicConstraints=critical,CA:TRUE" \
    -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
fi

san_marker="DNS:mqtt,DNS:localhost,IP:127.0.0.1,IP:$bind_ip"
registry="${PROXY_PRINTER_REGISTRY:-/registry/printers.json}"
if [ -s "$registry" ]; then
  for endpoint_ip in $(jq -r '.[] | select((.endpoint_enabled // true) and (.virtual_ip // "") != "") | .virtual_ip' "$registry"); do
    case "$endpoint_ip" in
      *[!0-9.]*|.*|*.) echo "Ignoring invalid endpoint IP: $endpoint_ip" >&2 ;;
      *) san_marker="$san_marker,IP:$endpoint_ip" ;;
    esac
  done
fi
previous_sans=""
if [ -f "$cert_dir/server.sans" ]; then
  previous_sans="$(sed -n '1p' "$cert_dir/server.sans")"
fi

if [ ! -s "$cert_dir/server.key" ] || [ ! -s "$cert_dir/server.crt" ] || [ "$previous_sans" != "$san_marker" ]; then
  echo "Generating TLS certificate for $bind_ip"
  openssl genrsa -out "$cert_dir/server.key" 3072 >/dev/null 2>&1
  openssl req -new -sha256 \
    -key "$cert_dir/server.key" \
    -out "$cert_dir/server.csr" \
    -subj "/CN=bambu-cloud-lan-proxy" >/dev/null 2>&1
  printf '%s\n' \
    'basicConstraints=critical,CA:FALSE' \
    'keyUsage=critical,digitalSignature,keyEncipherment' \
    'extendedKeyUsage=serverAuth' \
    "subjectAltName=$san_marker" > "$cert_dir/server.ext"
  openssl x509 -req -sha256 -days 825 \
    -in "$cert_dir/server.csr" \
    -CA "$cert_dir/ca.crt" \
    -CAkey "$cert_dir/ca.key" \
    -CAcreateserial \
    -out "$cert_dir/server.crt" \
    -extfile "$cert_dir/server.ext" >/dev/null 2>&1
  printf '%s\n' "$san_marker" > "$cert_dir/server.sans"
  rm -f "$cert_dir/server.csr" "$cert_dir/server.ext"
fi

if [ -f "$auth_dir/passwords" ]; then
  chown 0:0 "$auth_dir/passwords"
fi
mosquitto_passwd -b -c "$auth_dir/passwords" bblp "$PROXY_ACCESS_CODE"
cp "$cert_dir/ca.crt" "$export_dir/bambu-cloud-lan-proxy-ca.crt"
cp "$cert_dir/ca.crt" "$runtime_cert_dir/ca.crt"
cp "$cert_dir/server.crt" "$runtime_cert_dir/server.crt"
cp "$cert_dir/server.key" "$runtime_cert_dir/server.key"
cp "$auth_dir/passwords" "$runtime_auth_dir/passwords"

# Keep the CA signing key root-only. Runtime containers receive a separate
# directory containing only the server material and hashed password they need.
chown -R 0:0 "$cert_dir" "$auth_dir"
chmod 700 "$cert_dir" "$auth_dir"
chown -R "$runtime_uid":1883 "$runtime_dir"
chown -R "$runtime_uid":"$runtime_gid" "$camera_state_dir"
chmod 1777 "$local_status_dir"
chmod 750 "$runtime_dir" "$runtime_cert_dir" "$runtime_auth_dir"
chmod 700 "$camera_state_dir"
chmod 1777 "$remote_cache_dir"
find "$remote_cache_dir" -type f -mtime +1 -delete
chmod 755 "$export_dir"
chmod 600 "$cert_dir/ca.key" "$cert_dir/server.key" "$auth_dir/passwords"
chmod 644 "$cert_dir/ca.crt" "$cert_dir/server.crt" "$export_dir/bambu-cloud-lan-proxy-ca.crt"
chmod 640 "$runtime_cert_dir/server.key" "$runtime_auth_dir/passwords"
chmod 644 "$runtime_cert_dir/ca.crt" "$runtime_cert_dir/server.crt"

# The FTPS container runs as the same fixed numeric uid as Mosquitto. Staged
# files are world-readable so the unprivileged Gateway uid can snapshot them,
# while directory writes remain limited to the FTPS service.
chown -R "$runtime_uid":"$runtime_gid" "$staging_dir"
find "$staging_dir" -type d -exec chmod 755 {} \;
find "$staging_dir" -type f -exec chmod 644 {} \;

echo "Proxy TLS, MQTT credentials, and FTPS staging storage are ready"
