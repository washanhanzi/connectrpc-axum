# Development

## Claude Code Skills

This project includes [Claude Code skills](https://docs.anthropic.com/en/docs/claude-code/skills) to assist with development. See the skill files in [`.claude/skills/`](https://github.com/washanhanzi/connectrpc-axum/tree/main/.claude/skills/) for details.

| Skill | Description |
|-------|-------------|
| `submit-issue` | Handle questions, feature requests, and bug reports. Attempts to answer from documentation first, verifies bugs with tests, then submits GitHub issues when needed. |
| `test` | Run the complete test suite including unit tests, doc tests, and Go client integration tests. |
| `compare-repo` | Compare an external GitHub repository against connectrpc-axum. Analyzes features, architecture, and implementation to generate a comparison document. |
| `sync-arch-doc` | Sync architecture documentation with main branch changes. Tracks the `docs/arch` branch against `main` and updates architecture docs accordingly. |