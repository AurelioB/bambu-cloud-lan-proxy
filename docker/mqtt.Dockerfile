# syntax=docker/dockerfile:1
FROM eclipse-mosquitto:2@sha256:6f8d8a947c506f8a2290ec65cd4bd2bc7cb4d43fb5f6271f861cb013e2ef9797

ARG PROXY_VERSION="dev"
ARG PROXY_REVISION="unknown"
LABEL org.opencontainers.image.source="https://github.com/AurelioB/bambu-cloud-lan-proxy" \
      org.opencontainers.image.version="${PROXY_VERSION}" \
      org.opencontainers.image.revision="${PROXY_REVISION}"

COPY --chown=1883:1883 --chmod=0600 docker/mosquitto.conf /mosquitto/config/mosquitto.conf
COPY --chown=1883:1883 --chmod=0600 docker/proxy.acl /mosquitto/config/proxy.acl
