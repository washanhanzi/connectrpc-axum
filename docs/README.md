# Documentation

VitePress documentation for connectrpc-axum.

## Folder Structure

```
docs/
├── .vitepress/
│   ├── config.mts      # VitePress configuration (nav, sidebar, theme)
│   ├── theme/          # Custom theme components and styles
│   ├── cache/          # Build cache (gitignored)
│   └── dist/           # Built output (gitignored)
├── index.md            # Home page
├── guide/
│   ├── index.md        # Getting Started
│   ├── configuration.md
│   ├── limits.md
│   ├── timeout.md
│   ├── compression.md
│   ├── http-endpoints.md
│   ├── tonic.md
│   ├── grpc-web.md
│   ├── build.md
│   ├── examples.md
│   ├── development.md
│   ├── architecture.md
│   └── compare/
│       ├── axum-connect.md
│       └── connectrpc.md
└── plans/              # Internal planning docs (not in sidebar)
    ├── errordetails.md
    └── extended-codec-support.md
```

## Routes

| File | Route | Description |
|------|-------|-------------|
| `index.md` | `/` | Home page with hero section |
| `guide/index.md` | `/guide/` | Getting started, installation, basic usage |
| `guide/configuration.md` | `/guide/configuration` | MakeServiceBuilder API, service composition |
| `guide/limits.md` | `/guide/limits` | Receive and send message size limits |
| `guide/timeout.md` | `/guide/timeout` | Server-side timeout configuration |
| `guide/compression.md` | `/guide/compression` | Response compression (gzip) |
| `guide/http-endpoints.md` | `/guide/http-endpoints` | Plain HTTP routes alongside Connect |
| `guide/tonic.md` | `/guide/tonic` | Tonic gRPC integration, dual-protocol serving |
| `guide/grpc-web.md` | `/guide/grpc-web` | Browser gRPC-Web support via tonic-web |
| `guide/build.md` | `/guide/build` | build.rs config, prost, tonic codegen options |
| `guide/examples.md` | `/guide/examples` | Links to example code |
| `guide/development.md` | `/guide/development` | Contributing, Claude Code skills |
| `guide/architecture.md` | `/guide/architecture` | Library internals, request flow, module structure |
| `guide/compare/axum-connect.md` | `/guide/compare/axum-connect` | Comparison with axum-connect crate |
| `guide/compare/connectrpc.md` | `/guide/compare/connectrpc` | Comparison with connectrpc crate |

## Adding Pages

1. Create markdown file in appropriate folder
2. Add to sidebar in `.vitepress/config.mts` under `themeConfig.sidebar`

## Development

```bash
cd docs
npm install
npm run dev     # Start dev server
npm run build   # Build for production
```
