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

cert_dir=/data/certs
auth_dir=/data/auth
export_dir=/data/export
mkdir -p "$cert_dir" "$auth_dir" "$export_dir"

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

# The broker runs as the image's fixed mosquitto uid/gid.
chown -R 1883:1883 "$cert_dir" "$auth_dir"
chmod 700 "$cert_dir" "$auth_dir"
chmod 755 "$export_dir"
chmod 600 "$cert_dir/ca.key" "$cert_dir/server.key" "$auth_dir/passwords"
chmod 644 "$cert_dir/ca.crt" "$cert_dir/server.crt" "$export_dir/bambu-cloud-lan-proxy-ca.crt"

echo "Proxy TLS and MQTT credentials are ready"
