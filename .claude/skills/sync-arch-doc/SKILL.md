---
name: sync-arch-docs
description: Sync architecture documentation with main branch changes. This skill should be used when the user wants to update docs/guide/architecture.md to reflect changes made on the main branch. It compares origin/docs/arch against local main, analyzes new commits, and updates the architecture document.
---

# Sync Architecture Docs

## Overview

This skill synchronizes architecture documentation by comparing `origin/docs/arch` against local `main`. It identifies new commits, analyzes what changed, and updates the architecture document if needed. No git operations (merge, commit, push) are performed after the update.

## Workflow

### Step 1: Fetch and Find Missing Commits

Fetch latest from origin and find commits that exist in local `main` but not in origin/docs/arch:

```bash
git fetch origin
git log --oneline origin/docs/arch..main
```

If no commits are found (origin/docs/arch is up-to-date with local main), report this and exit.

### Step 2: Analyze Commit Changes

For each commit behind, examine the changes to understand their architectural impact:

```bash
# Get detailed diff for commits
git log --stat -p origin/docs/arch..main

# Focus on key files that affect architecture
git diff origin/docs/arch..main -- src/ Cargo.toml
```

Look for changes that affect:
- Module structure (new/removed modules)
- Public APIs (new types, traits, functions)
- Dependencies (Cargo.toml changes)
- Handler patterns
- Request/response flow
- Protocol handling
- Code generation

### Step 3: Read Current Architecture Doc

Read the existing architecture document:

```bash
cat docs/guide/architecture.md
```

Compare against the analyzed changes to identify gaps.

### Step 4: Update Architecture Document

If updates are needed, edit `docs/guide/architecture.md` to reflect:
- New modules or components
- Changed APIs or patterns
- Updated workflows or data flows
- New design decisions

Keep the document concise and focused on architectural understanding.

## Checklist

- [ ] Fetch origin and check for commits behind (local main vs origin/docs/arch)
- [ ] Analyze commit changes for architectural impact
- [ ] Read current architecture document
- [ ] Update architecture document if needed
