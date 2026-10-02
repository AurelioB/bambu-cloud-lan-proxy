# Public website

An independent static landing page and searchable documentation site. It never
connects to the admin API and does not need account or printer credentials.

```sh
cd website
npm ci
npm run dev
```

Build/check: `npm run check && npm run build`. Browser tests: install Chromium
with `npx playwright install chromium`, then `npm test` after a build.

Output: `website/dist/`. Deploy only this directory. `SITE_URL` is the canonical
origin and `SITE_BASE` is `/` or a trailing-slash project path. The admin UI is
still built separately from `web/` and its authentication is unchanged.

Authentication content is synced from `docs/admin-authentication.md` so there is
only one security contract. Downloads are synced from `examples/`. Generated
copies are ignored by Git. The build checks local links/anchors and HTTP route
coverage against `website/public/openapi.json`.

The landing page's console screenshot uses synthetic demo printers. It is not a
live dashboard. The site has local fonts/search and no analytics or remote font
dependencies. Astro/Starlight supplies documentation navigation, code-copy
buttons, search, and accessible responsive layouts.

The landing page's `RequestFlow.astro` component is a local-only animated
explainer. It illustrates file/camera/control/status/staging routes, with
optional cloud fallbacks for files and telemetry. It never sends real printer
requests. Playback pauses offscreen or in hidden tabs, starts paused for reduced
motion, and includes an explicit play/pause control. Without JavaScript, the
local file route and its explanation remain available.

To serve the public site in Docker, from the repository root:

```sh
docker build -f website/Dockerfile -t bambu-proxy-docs .
docker run --rm -p 127.0.0.1:8080:8080 bambu-proxy-docs
```

Use `SITE_BASE=/` for this container's root-hosted deployment; project-path
deployments need a matching web-server mount/location. This image is unrelated
to the actual printer proxy's Docker/Compose installation.

Publishing is opt-in via `.github/workflows/docs-deploy.yaml`. Set the repository
variables `DOCS_SITE_URL` and `DOCS_BASE_PATH`, configure Pages, and manually run
the workflow on the intended revision. It does not change repository visibility.
The contributor guide lists the pre-publication checks and hosting examples.
