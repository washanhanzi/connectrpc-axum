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

### Step 5: Write Comparison Document

Create output at `docs/guide/compare/<repo-name>.md` with the following structure:

```markdown
# Comparison: connectrpc-axum vs <repo-name>

## Overview

Brief description of what <repo-name> is and its primary purpose.

## Feature Comparison

### Supported Protocols
[Narrative comparing protocol support]

### API Design
[Narrative comparing how users define services/handlers]

### Streaming Support
[Narrative comparing streaming capabilities]

### Error Handling
[Narrative comparing error handling approaches]

## Technical Implementation

### Architecture Patterns
[Narrative comparing architectural approaches]

### Core Abstractions
[Narrative comparing key types and abstractions]

### Middleware/Layer Design
[Narrative comparing extensibility patterns]

### Code Generation
[Narrative comparing code generation approaches, if applicable]

## Quality & Developer Experience

### Documentation
[Narrative comparing documentation quality and completeness]

### Examples
[Narrative comparing example coverage]

### Testing
[Narrative comparing test organization and coverage]

## Summary

### Where connectrpc-axum Excels
[Key strengths of our library]

### Where <repo-name> Excels
[Key strengths of the compared library]

### Key Takeaways
[Important learnings and potential improvements]
```

## Output

The comparison document is created at:
```
docs/guide/compare/<repo-name>.md
```

Ensure the `docs/guide/compare/` directory exists before writing.

## Notes

- Be objective and fair in comparisons
- Focus on factual differences rather than subjective judgments
- Identify potential improvements for connectrpc-axum
- Note any unique features or approaches worth learning from
