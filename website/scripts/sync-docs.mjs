import { mkdir, readFile, writeFile, cp } from 'node:fs/promises';

// The deployed font files must retain their SIL Open Font License notice.
await cp(
  new URL('../node_modules/@fontsource-variable/inter/LICENSE', import.meta.url),
  new URL('../public/inter-font-LICENSE.txt', import.meta.url),
);

// One canonical authentication guide: never maintain two security contracts.
const source = await readFile(
  new URL('../../docs/admin-authentication.md', import.meta.url),
  'utf8',
);
const target = new URL(
  '../src/content/docs/docs/authentication.md',
  import.meta.url,
);
await mkdir(new URL('../src/content/docs/docs/', import.meta.url), {
  recursive: true,
});
await writeFile(
  target,
  '---\ntitle: Admin authentication & OIDC\ndescription: Protect the administration UI without changing printer-client compatibility.\n---\n' +
    source.replace(/^# Admin authentication\n/, ''),
);
await mkdir(new URL('../public/examples/', import.meta.url), {
  recursive: true,
});
await cp(
  new URL('../../examples/', import.meta.url),
  new URL('../public/examples/', import.meta.url),
  { recursive: true },
);
