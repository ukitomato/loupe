import { defineConfig } from 'vitepress'

export default defineConfig({
  base: '/indexify/',
  title: 'indexify',
  description: 'Fast indexed full-text code search — one n-gram index, three front-ends: CLI, MCP server, and VS Code.',

  head: [
    ['link', { rel: 'icon', href: '/indexify/favicon.svg', type: 'image/svg+xml' }],
  ],

  themeConfig: {
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Reference', link: '/reference/cli' },
      { text: 'VS Code', link: '/reference/vscode' },
      { text: 'Changelog', link: '/changelog' },
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Guide',
          items: [
            { text: 'Getting Started', link: '/guide/getting-started' },
            { text: 'How It Works', link: '/guide/how-it-works' },
          ],
        },
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'CLI', link: '/reference/cli' },
            { text: 'MCP Server', link: '/reference/mcp-server' },
            { text: 'VS Code Extension', link: '/reference/vscode' },
            { text: 'Configuration', link: '/reference/configuration' },
          ],
        },
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/ukitomato/indexify' },
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Built on <a href="https://github.com/quickwit-oss/tantivy">Tantivy</a> (MIT)',
    },

    search: {
      provider: 'local',
    },
  },
})
