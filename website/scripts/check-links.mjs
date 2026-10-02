import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';

const root = path.resolve('dist');
const base = (process.env.SITE_BASE || '/').replace(/\/$/, '');
const origin = 'https://docs.invalid';
const files = await readdir(root, { recursive: true });
const pages = files.filter((file) => file.endsWith('.html'));
let checked = 0;
const failures = [];
for (const file of pages) {
  const html = await readFile(path.join(root, file), 'utf8');
  const source = `${base}/${file.replace(/index\.html$/, '')}`;
  for (const [, raw] of html.matchAll(/(?:href|src)="([^"]+)"/g)) {
    if (/^(?:data:|mailto:|tel:|javascript:)/.test(raw)) continue;
    const url = new URL(raw.replaceAll('&amp;', '&'), `${origin}${source}`);
    if (url.origin !== origin) continue;
    let pathname = decodeURIComponent(url.pathname);
    if (base && pathname !== base && !pathname.startsWith(`${base}/`)) {
      failures.push(`${file}: link escaped base path: ${raw}`);
      continue;
    }
    pathname = pathname.slice(base.length).replace(/^\//, '');
    let target = path.join(root, pathname);
    if (!target.startsWith(root + path.sep) && target !== root)
      throw new Error('Unsafe link path');
    try {
      if ((await stat(target)).isDirectory())
        target = path.join(target, 'index.html');
      await stat(target);
      if (url.hash && target.endsWith('.html')) {
        const targetHtml = await readFile(target, 'utf8');
        const anchor = decodeURIComponent(url.hash.slice(1));
        if (!targetHtml.includes(`id="${anchor}"`))
          throw new Error(`missing anchor ${anchor}`);
      }
      checked++;
    } catch (error) {
      failures.push(`${file}: ${raw}: ${error.message}`);
    }
  }
}

// Route/method drift checks keep the reference tied to the real routers.
const spec = JSON.parse(await readFile('public/openapi.json', 'utf8'));
const routes = new Map();
for (const sourceFile of ['../src/web.rs', '../src/admin_auth.rs']) {
  const source = (await readFile(sourceFile, 'utf8')).replaceAll(
    'axum::routing::',
    '',
  );
  for (const match of source.matchAll(
    /\.route\(\s*"((?:\/api\/|\/internal\/)[^"]+)"\s*,\s*((?:get|post|put)\([^)]*\)(?:\.(?:get|post|put)\([^)]*\))*)/g,
  )) {
    routes.set(
      match[1],
      [...match[2].matchAll(/(?:^|\.)(get|post|put)\(/g)].map((m) => m[1]),
    );
  }
}
assert(routes.size >= 15, 'Route scanner did not find the expected routers');
assert.deepEqual(
  Object.keys(spec.paths).sort(),
  [...routes.keys()].sort(),
  'Document every HTTP API path',
);
for (const [route, methods] of routes) {
  assert.deepEqual(
    Object.keys(spec.paths[route]).sort(),
    methods.sort(),
    `Methods drifted at ${route}`,
  );
}
function references(node) {
  if (!node || typeof node !== 'object') return;
  if (node.$ref) {
    const value = node.$ref
      .replace(/^#\//, '')
      .split('/')
      .reduce((o, key) => o?.[key], spec);
    assert(value, `Missing OpenAPI reference ${node.$ref}`);
  }
  Object.values(node).forEach(references);
}
references(spec);
const contentRoot = 'src/content/docs';
for (const file of await readdir(contentRoot, { recursive: true })) {
  if (!file.endsWith('.md')) continue;
  const markdown = await readFile(path.join(contentRoot, file), 'utf8');
  for (const [, block] of markdown.matchAll(/```(?:sh|bash)\n([\s\S]*?)```/g)) {
    // Syntax-check examples only. Never execute documented install/printer actions.
    execFileSync('bash', ['-n'], {
      input: block,
      stdio: ['pipe', 'pipe', 'pipe'],
    });
  }
}
if (failures.length) throw new Error(failures.join('\n'));
console.log(
  `Checked ${checked} local links/assets across ${pages.length} pages, ${routes.size} HTTP API paths, and shell example syntax.`,
);
