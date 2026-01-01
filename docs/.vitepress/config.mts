import { defineConfig } from 'vitepress'

export default defineConfig({
  title: "connectrpc-axum",
  description: "ConnectRPC protocol implementation for Axum",

  markdown: {
    theme: {
      light: 'github-light',
      dark: 'github-dark'
    },
    codeTransformers: [
      {
        name: 'comment-color',
        span(node) {
          const style = node.properties?.style?.toString() || ''
          if (style.includes('--shiki-dark:#6A737D')) {
            node.properties.style = style.replace('--shiki-dark:#6A737D', '--shiki-dark:#76FF03')
          }
        }
      }
    ]
  },

  head: [
    ['link', { rel: 'preconnect', href: 'https://fonts.googleapis.com' }],
    ['link', { rel: 'preconnect', href: 'https://fonts.gstatic.com', crossorigin: '' }],
    ['link', { href: 'https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap', rel: 'stylesheet' }],
  ],

  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/' }
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Guide',
          items: [
            { text: 'Getting Started', link: '/guide/' },
            { text: 'MakeServiceBuilder', link: '/guide/configuration' },
            { text: 'HTTP Endpoints', link: '/guide/http-endpoints' },
            { text: 'Tonic gRPC', link: '/guide/tonic' },
            { text: 'build.rs', link: '/guide/build' },
            { text: 'Examples', link: '/guide/examples' },
            { text: 'Development', link: '/guide/development' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/phlx-io/connectrpc-axum' }
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Built with VitePress'
    },

    search: {
      provider: 'local'
    },

    editLink: {
      pattern: 'https://github.com/phlx-io/connectrpc-axum/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    }
  }
})
