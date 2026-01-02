---
name: sync-arch-docs
description: Sync architecture documentation with main branch changes. This skill should be used when the user wants to update docs/guide/architecture.md to reflect changes made on the main branch. It tracks the docs/arch branch against origin/main, analyzes new commits, and updates the architecture document accordingly.
---

# Sync Architecture Docs

## Overview

This skill synchronizes architecture documentation by tracking a dedicated `docs/arch` branch against `origin/main`. It identifies new commits, analyzes what changed, determines if the architecture document needs updates, and pushes the updated branch.

## Workflow

### Step 1: Fetch and Find Missing Commits

Fetch latest from origin and find commits that exist in `origin/main` but not in the docs branch:

```bash
git fetch origin
git log --oneline origin/docs/arch..origin/main
```

If no commits are found (docs/arch is up-to-date), report this and exit.

### Step 2: Analyze Commit Changes

For each commit behind, examine the changes to understand their architectural impact:

```bash
# Get detailed diff for commits
git log --stat -p origin/docs/arch..origin/main

# Focus on key files that affect architecture
git diff origin/docs/arch..origin/main -- src/ Cargo.toml
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

### Step 5: Merge Main into docs/arch

Bring in the latest changes from main:

```bash
git merge origin/main -m "chore: sync with main"
```

### Step 6: Commit and Push

If architecture doc was updated:

```bash
git add docs/guide/architecture.md
git commit -m "docs: update architecture for recent changes"
```

Push the updated branch:

```bash
git push origin docs/arch
```

## Checklist

- [ ] Fetch origin and check for commits behind
- [ ] Analyze commit changes for architectural impact
- [ ] Read current architecture document
- [ ] Update architecture document if needed
- [ ] Merge main into docs/arch
- [ ] Push docs/arch to origin