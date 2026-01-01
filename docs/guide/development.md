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

If not using Claude Code, see the corresponding skill files in [`.claude/skills/`](https://github.com/phlx-io/connectrpc-axum/tree/main/.claude/skills/) for instructions.

## Architecture

See [`.claude/architecture.md`](https://github.com/phlx-io/connectrpc-axum/blob/main/.claude/architecture.md) for detailed documentation on the project structure, core modules, and design decisions.
