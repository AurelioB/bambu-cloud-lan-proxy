---
title: Contributing & publishing
description: Build the project, maintain documentation, and publish the static community site.
---
## Contribute safely

Open a focused issue or pull request in the [project repository](https://github.com/AurelioB/bambu-cloud-lan-proxy). Describe the model/firmware/client combination and distinguish observed behavior from an inferred capability profile.

Never commit real serials, access codes, Bambu callbacks/tokens, private keys, plugin state, or unredacted logs. Tests and screenshots should use clearly identified demo data.

Core code is Rust; the official plugin ABI is isolated in `native/`. The authenticated console is Svelte 5/shadcn-svelte in `web/`. The public site is independently built with Astro/Starlight in `website/`; it never needs printer or account credentials.

## Run checks

```sh
npm --prefix web ci
npm --prefix web run check
npm --prefix web run build
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
npm --prefix website ci
npm --prefix website run check
npm --prefix website run build
```

For browser checks, install Playwright Chromium in the relevant project and run its test script. Website tests use a static preview only, with no printer connection.

## Edit documentation

Public pages live in `website/src/content/docs/docs/`. The authentication page is generated from the canonical `docs/admin-authentication.md` by `npm run sync`; edit the canonical file instead. Deployment examples under `examples/` are copied into the static output for downloads.

Add pages to the sidebar and use root-relative Markdown links such as `/docs/features/`. The build prefixes them with `SITE_BASE`, checks local links/anchors, and builds a local search index. No hosted search account or analytics is required.

Update examples alongside implementation changes. HTTP routes are checked against the bundled OpenAPI reference during the website build.

## Preview the public site

```sh
cd website
npm ci
npm run dev -- --host 127.0.0.1
```

The command prints the local URL. For a production preview:

```sh
npm run build
npm run preview -- --host 127.0.0.1
```

## Publish static files

The website does not deploy the printer proxy or expose its private admin UI. Publish **only `website/dist/`**, never the repository directory or runtime `data/`.

For a custom domain at its root:

```sh
cd website
SITE_URL=https://your-docs.example SITE_BASE=/ npm run build
```

For a GitHub Pages project URL:

```sh
cd website
SITE_URL=https://your-user.github.io \
  SITE_BASE=/bambu-cloud-lan-proxy/ npm run build
```

Keep the trailing slash on `SITE_BASE`. Site URLs here are examples, not claims that a live site has already been published.

The repository includes a manual **Publish documentation** Actions workflow. Configure repository variables `DOCS_SITE_URL` and `DOCS_BASE_PATH`, enable Pages with GitHub Actions as the source, and run the workflow on your reviewed publication revision. The workflow is deliberately not an automatic deployment on every push.

Before sharing publicly, confirm repository visibility/access, remove private operational material, review the source license and proprietary plugin notices, and verify clone instructions from an unauthenticated browser. Changing repository visibility is an owner decision, not a build step.

You can also serve the output from any static web host. A container example for the **documentation site only** is in `website/Dockerfile`.

## Release checklist

- Source revision and docs agree; newly documented auth features exist in that revision.
- Linux x86-64 requirements and unsupported camera types are visible.
- Compose example validates; source builds and tests pass.
- Links, search, keyboard navigation, mobile layout, and light/dark appearance work.
- A fresh-host smoke test is recorded for each deployment method advertised as validated.
- Public images and examples contain only demo data.
- No public image registry or downloadable binary is advertised unless that artifact actually exists.
