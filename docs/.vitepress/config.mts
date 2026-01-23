import { defineConfig } from 'vitepress'

export default defineConfig({
  title: "connectrpc-axum",
  description: "ConnectRPC protocol implementation for Axum",
  base: '/connectrpc-axum/',

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
      { text: 'Guide', link: '/guide/' },
      { text: 'Blog', link: '/blog/origin' }
    ],

    sidebar: {
      '/blog/': [
        {
          text: 'Blog',
          items: [
            { text: 'Origin', link: '/blog/origin' }
          ]
        }
      ],
      '/guide/': [
        {
          text: 'Guide',
          items: [
            { text: 'Getting Started', link: '/guide/' },
            {
              text: 'MakeServiceBuilder',
              link: '/guide/configuration',
              items: [
                { text: 'Message Limits', link: '/guide/limits' },
                { text: 'Timeout', link: '/guide/timeout' },
                { text: 'Compression', link: '/guide/compression' }
              ]
            },
            { text: 'Axum Router', link: '/guide/axum-router' },
            {
              text: 'Tonic gRPC',
              link: '/guide/tonic',
              items: [
                { text: 'gRPC-Web', link: '/guide/grpc-web' }
              ]
            },
            { text: 'build.rs', link: '/guide/build' },
            { text: 'Examples', link: '/guide/examples' },
            { text: 'Development', link: '/guide/development' },
            { text: 'Architecture', link: '/guide/architecture' }
          ]
        },
        {
          text: 'Comparisons',
          items: [
            { text: 'axum-connect', link: '/guide/compare/axum-connect' },
            { text: 'connectrpc', link: '/guide/compare/connectrpc' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/washanhanzi/connectrpc-axum' }
    ],

    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Built with VitePress'
    },

    search: {
      provider: 'local'
    },

    editLink: {
      pattern: 'https://github.com/washanhanzi/connectrpc-axum/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    }
  }
})
