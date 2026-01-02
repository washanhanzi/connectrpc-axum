# Development

## Claude Code Slash Commands

This project provides [slash commands](https://docs.anthropic.com/en/docs/claude-code/slash-commands) for common development tasks:

| Command | Description |
|---------|-------------|
| `/connectrpc-axum:submit-issue` | Report bugs, request features, or ask questions |
| `/connectrpc-axum:test` | Run the full test suite |

Usage:

```bash
claude /connectrpc-axum:submit-issue "Description of your issue or feature request"
claude /connectrpc-axum:test
```

If not using Claude Code, see the corresponding skill files in [`.claude/skills/`](https://github.com/washanhanzi/connectrpc-axum/tree/main/.claude/skills/) for instructions.

## Project Skills

This project includes several [Claude Code skills](https://docs.anthropic.com/en/docs/claude-code/skills) to assist with development:

### User-Invocable Skills

| Skill | Description |
|-------|-------------|
| `submit-issue` | Handle questions, feature requests, and bug reports. Attempts to answer from documentation first, verifies bugs with tests, then submits GitHub issues when needed. |
| `test` | Run the complete test suite including unit tests, doc tests, and Go client integration tests. |

### Reference Skills

These skills are used automatically by Claude when relevant:

| Skill | Description |
|-------|-------------|
| `connect-go-reference` | Reference the local `connect-go/` directory for ConnectRPC protocol details. Always uses local files instead of fetching from GitHub. |
| `architecture` | Quick reference to project architecture at `docs/guide/architecture.md`. Use when understanding codebase structure, module organization, or key types. |
| `sync-arch-doc` | Sync architecture documentation with main branch changes. Tracks the `docs/arch` branch against `origin/main` and updates architecture docs accordingly. |