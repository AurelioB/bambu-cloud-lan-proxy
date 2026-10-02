# syntax=docker/dockerfile:1
FROM alpine:3.23@sha256:fd791d74b68913cbb027c6546007b3f0d3bc45125f797758156952bc2d6daf40

ARG PROXY_VERSION="dev"
ARG PROXY_REVISION="unknown"
LABEL org.opencontainers.image.source="https://github.com/AurelioB/bambu-cloud-lan-proxy" \
      org.opencontainers.image.version="${PROXY_VERSION}" \
      org.opencontainers.image.revision="${PROXY_REVISION}"

RUN apk add --no-cache jq mosquitto openssl
COPY docker/bootstrap.sh /usr/local/bin/bootstrap
ENTRYPOINT ["/usr/local/bin/bootstrap"]
