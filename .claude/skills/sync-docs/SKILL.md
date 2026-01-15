---
name: sync-docs
description: Sync documentation with main branch changes. This skill should be used when the user wants to update VitePress documentation to reflect changes made on the main branch. It compares origin/docs against local main, analyzes new commits, and updates relevant documentation files in parallel.
---

# Sync Docs

## Overview

This skill synchronizes VitePress documentation by comparing `origin/docs` against local `main`. It identifies new commits, analyzes what changed, and updates relevant documentation files. No git operations (merge, commit, push) are performed after the update.

## Workflow

### Step 1: Fetch and Find Missing Commits

Fetch latest from origin and find commits that exist in local `main` but not in origin/docs:

```bash
git fetch origin
git log --oneline origin/docs..main
```

If no commits are found (origin/docs is up-to-date with local main), report this and exit.

### Step 2: Analyze Commit Changes

Get the diff to understand what changed:

```bash
git log --stat -p origin/docs..main
```

### Step 3: Read Documentation Index

Read the docs README to understand the documentation structure:

```bash
cat docs/README.md
```

This provides the route mapping:

| File | Description |
|------|-------------|
| `guide/index.md` | Getting started, installation, basic usage |
| `guide/configuration.md` | MakeServiceBuilder API, service composition |
| `guide/timeout.md` | Server-side timeout configuration |
| `guide/compression.md` | Response compression (gzip) |
| `guide/http-endpoints.md` | Plain HTTP routes alongside Connect |
| `guide/tonic.md` | Tonic gRPC integration, dual-protocol serving |
| `guide/grpc-web.md` | Browser gRPC-Web support via tonic-web |
| `guide/build.md` | build.rs config, prost, tonic codegen options |
| `guide/examples.md` | Links to example code |
| `guide/development.md` | Contributing, Claude Code skills |
| `guide/architecture.md` | Library internals, request flow, module structure |

### Step 4: Update Documentation in Parallel

Launch two subagents in parallel:

1. **Architecture subagent** - Updates `docs/guide/architecture.md` for:
   - Module structure changes
   - Public API changes
   - Request/response flow changes
   - Code generation changes

2. **Guide subagent** - Updates other relevant guide files based on commit changes:
   - `build.md` - for build/codegen changes
   - `configuration.md` - for MakeServiceBuilder changes
   - `tonic.md` - for gRPC integration changes
   - `compression.md` - for compression changes
   - Other files as needed based on the diff

Each subagent should:
- Read the current doc file
- Compare against the commit changes
- Update only if the doc is outdated or missing information
- Keep documentation concise and user-focused (not implementation details)

## Checklist

- [ ] Fetch origin and check for commits behind (local main vs origin/docs)
- [ ] Analyze commit changes
- [ ] Read docs/README.md for route mapping
- [ ] Launch architecture subagent
- [ ] Launch guide subagent for other relevant docs
- [ ] Report which files were updated
