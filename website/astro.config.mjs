import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { unified } from '@astrojs/markdown-remark';

const base = process.env.SITE_BASE || '/';

// Keep Markdown links portable between a custom domain and a Pages project path.
function baseLinks() {
  return (tree) => {
    const visit = (node) => {
      if (
        typeof node.url === 'string' &&
        node.url.startsWith('/') &&
        !node.url.startsWith('//')
      ) {
        node.url = `${base.replace(/\/$/, '')}${node.url}`;
      }
      node.children?.forEach(visit);
    };
    visit(tree);
  };
}

export default defineConfig({
  site: process.env.SITE_URL || 'https://aureliob.github.io',
  base,
  trailingSlash: 'always',
  markdown: { processor: unified({ remarkPlugins: [baseLinks] }) },
  integrations: [
    starlight({
      title: 'Bambu Cloud LAN Proxy',
      description:
        'Keep Bambu Handy. Connect your local tools. Self-host a virtual LAN endpoint for each printer.',
      favicon: '/favicon.svg',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/AurelioB/bambu-cloud-lan-proxy',
        },
      ],
      customCss: ['./src/styles/docs.css'],
      expressiveCode: {
        plugins: [
          {
            name: 'keyboard-scrollable-code-groups',
            hooks: {
              postprocessRenderedBlock: ({ renderData }) => {
                const visit = (node) => {
                  if (node.type === 'element' && node.tagName === 'pre') {
                    node.properties.tabIndex = 0;
                    node.properties.role = 'group';
                    node.properties.ariaLabel = 'Code example';
                  }
                  node.children?.forEach(visit);
                };
                visit(renderData.blockAst);
              },
            },
          },
        ],
      },
      sidebar: [
        {
          label: 'Start here',
          items: [
            { label: 'Overview', slug: 'docs' },
            {
              label: 'Requirements & compatibility',
              slug: 'docs/compatibility',
            },
            { label: 'First printer', slug: 'docs/first-printer' },
          ],
        },
        {
          label: 'Installation',
          items: [
            { label: 'Releases & prebuilt installation', slug: 'docs/releases' },
            {
              label: 'Docker Compose · recommended',
              slug: 'docs/install/compose',
            },
            { label: 'Docker without Compose', slug: 'docs/install/docker' },
            { label: 'Standalone Linux', slug: 'docs/install/standalone' },
          ],
        },
        {
          label: 'Use your proxy',
          items: [
            { label: 'Features & data paths', slug: 'docs/features' },
            { label: 'Bambuddy & LAN clients', slug: 'docs/clients' },
            {
              label: 'Multiple printers & networking',
              slug: 'docs/networking',
            },
            { label: 'Permissions & printing', slug: 'docs/permissions' },
            {
              label: 'Admin authentication & OIDC',
              slug: 'docs/authentication',
            },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Configuration', slug: 'docs/configuration' },
            { label: 'HTTP API', slug: 'docs/api' },
            { label: 'CLI & LAN protocols', slug: 'docs/protocols' },
          ],
        },
        {
          label: 'Maintain & contribute',
          items: [
            { label: 'Updates, backups & security', slug: 'docs/operations' },
            { label: 'Troubleshooting', slug: 'docs/troubleshooting' },
            { label: 'FAQ', slug: 'docs/faq' },
            { label: 'Contributing & publishing', slug: 'docs/contributing' },
          ],
        },
      ],
    }),
  ],
});
