# Build from the repository root: docker build -f website/Dockerfile -t bambu-proxy-docs .
# This image serves PUBLIC DOCUMENTATION ONLY, not the printer proxy.
FROM node:24-bookworm-slim@sha256:0e0ff40c39bc087845bfb27465a0df4ea419520094bc35842ff83dd8cbe6f9b6 AS build
WORKDIR /src/website
COPY website/package.json website/package-lock.json ./
RUN npm ci
COPY website/ ./
COPY docs/admin-authentication.md /src/docs/admin-authentication.md
COPY examples/ /src/examples/
COPY src/web.rs src/admin_auth.rs /src/src/
ARG SITE_URL=https://aureliob.github.io
ARG SITE_BASE=/
ENV SITE_URL=$SITE_URL SITE_BASE=$SITE_BASE ASTRO_TELEMETRY_DISABLED=1
RUN npm run build

FROM nginxinc/nginx-unprivileged:stable-alpine@sha256:4714e0b1b2577eaa1a6131d07c958b67f0eb68e6d0521e90c6e5287db8cf0bc5
COPY --from=build /src/website/dist /usr/share/nginx/html
EXPOSE 8080
