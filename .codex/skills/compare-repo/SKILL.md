---
name: compare-repo
description: Compare a GitHub repository with connectrpc-axum. This skill should be used when the user provides a GitHub repository URL and wants to compare it with the current project. Analyzes user features, technical implementation, architecture patterns, code quality, tests, and documentation. Outputs comparison to docs/guide/compare/ directory.
---

# Compare Repository Skill

Compare an external GitHub repository against connectrpc-axum to understand differences in features, architecture, and implementation approaches.

## Prerequisites

Before starting, ensure:
- The user has provided a GitHub repository URL
- The current working directory is the connectrpc-axum project root

## Workflow

### Step 1: Extract Repository Information

Parse the GitHub URL to extract:
- Repository owner
- Repository name (used for folder and output file naming)

Example: `https://github.com/owner/repo-name` → `repo-name`

### Step 2: Prepare Compare Directory

```bash
# Remove existing clone if present (ensures latest version)
rm -rf compare/<repo-name>

# Create compare directory if it doesn't exist
mkdir -p compare

# Clone the repository
git clone <github-url> compare/<repo-name>
```

### Step 3: Read Our Architecture

Read `docs/guide/architecture.md` to understand connectrpc-axum's:
- Core design principles
- Request lifecycle
- Module organization
- Key types and patterns

### Step 4: Analyze Target Repository

Explore the cloned repository to understand:

**Project Structure:**
- Identify main source directories
- Find configuration files (Cargo.toml, package.json, etc.)
- Locate documentation and examples

**Core Features:**
- What protocols/formats does it support?
- What is the API surface for users?
- How are handlers/services defined?

**Technical Implementation:**
- Architecture patterns used
- Key abstractions and types
- Error handling approach
- Streaming support
- Middleware/layer design

**Quality Indicators:**
- Test coverage and organization
- Documentation quality
- Example completeness
- Code organization

### Step 5: Get Repository Commit Hash

Get the current commit hash of the cloned repository for the front matter:

```bash
git -C compare/<repo-name> rev-parse HEAD
```

### Step 6: Write Comparison Document

Create output at `docs/guide/compare/<repo-name>.md` with the following structure:

```markdown
---
title: Comparison with <repo-name>
repo: <github-url>
commit: <commit-hash>
date: <YYYY-MM-DD>
author: Claude Opus 4.5
---

# Comparison: connectrpc-axum vs <repo-name>

<ComparisonMeta />

## Overview

Brief description of what <repo-name> is, its primary purpose, and how it relates to connectrpc-axum (2-3 paragraphs max).

## Feature Comparison

For each feature category, use bullet points showing what each framework provides, followed by a short paragraph describing significant differences.

### Protocols & Encodings

**connectrpc-axum:**
- [Feature 1]
- [Feature 2]

**<repo-name>:**
- [Feature 1]
- [Feature 2]

[1-2 sentences on key differences]

### Streaming

**connectrpc-axum:**
- [Streaming capabilities]

**<repo-name>:**
- [Streaming capabilities]

[1-2 sentences on key differences]

### Compression & Performance

**connectrpc-axum:**
- [Compression/performance features]

**<repo-name>:**
- [Compression/performance features]

[1-2 sentences on key differences]

### Error Handling

**connectrpc-axum:**
- [Error handling approach]

**<repo-name>:**
- [Error handling approach]

[1-2 sentences on key differences]

## API Design

Compare handler signatures and how users define services. Include code examples from both libraries showing equivalent functionality.

**connectrpc-axum:**
```rust
// Example handler signature
```

**<repo-name>:**
```rust
// Example handler signature
```

[Brief explanation of the design philosophy differences]

## Implementation Details

Keep this section concise - only the key architectural points.

| Aspect | connectrpc-axum | <repo-name> |
|--------|-----------------|-------------|
| Architecture | [1-2 words] | [1-2 words] |
| Request handling | [1-2 words] | [1-2 words] |
| Middleware | [1-2 words] | [1-2 words] |
| Code generation | [1-2 words] | [1-2 words] |

[Optional: 1-2 sentences on significant architectural differences worth noting]

## Summary

**connectrpc-axum strengths:** [bullet list, 3-5 items]

**<repo-name> strengths:** [bullet list, 3-5 items]

**Key takeaways:** [2-3 bullet points on learnings or potential improvements]
```

### Step 7: Update VitePress Navigation

Add the new comparison page to the VitePress sidebar in `docs/.vitepress/config.mts`.

Find the `Comparisons` section in the sidebar configuration and add a new entry:

```typescript
{
  text: 'Comparisons',
  items: [
    // ... existing items
    { text: 'vs <repo-name>', link: '/guide/compare/<repo-name>' }
  ]
}
```

If the `Comparisons` section doesn't exist, create it after the Guide section:

```typescript
{
  text: 'Comparisons',
  items: [
    { text: 'vs <repo-name>', link: '/guide/compare/<repo-name>' }
  ]
}
```

## Output

The comparison document is created at:
```
docs/guide/compare/<repo-name>.md
```

The VitePress navigation is updated at:
```
docs/.vitepress/config.mts
```

Ensure the `docs/guide/compare/` directory exists before writing.

## Notes

- Be objective and fair in comparisons
- Focus on factual differences rather than subjective judgments
- Identify potential improvements for connectrpc-axum
- Note any unique features or approaches worth learning from
