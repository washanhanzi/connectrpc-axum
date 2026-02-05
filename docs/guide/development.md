# Development

## Claude Code Skills

This project includes [Claude Code skills](https://docs.anthropic.com/en/docs/claude-code/skills) to assist with development. See the skill files in [`.claude/skills/`](https://github.com/washanhanzi/connectrpc-axum/tree/main/.claude/skills/) for details.

| Skill | Description |
|-------|-------------|
| `architecture` | Reference for project architecture. Use when you need to understand the codebase structure, module organization, request/response flow, or key types. |
| `compare-repo` | Compare an external GitHub repository against connectrpc-axum. Analyzes features, architecture, and implementation to generate a comparison document. |
| `connect-go-reference` | Reference the local `connect-go/` directory for ConnectRPC protocol implementation details. |
| `create-integration-test` | Create integration tests for connectrpc-axum (Rust client tests or Rust server tests). |
| `resolve-issue` | Investigate and resolve GitHub issues. Analyzes the issue, references architecture docs and connect-go implementation, then posts a resolution plan. |
| `submit-issue` | Handle questions, feature requests, and bug reports. Attempts to answer from documentation first, verifies bugs with tests, then submits GitHub issues when needed. |
| `sync-docs` | Sync VitePress documentation with main branch changes. Compares origin/docs against local main, analyzes new commits, and updates relevant documentation files. |
| `test` | Run the complete test suite including unit tests, doc tests, and integration tests. |
| `tonic-client-reference` | Reference the local `tonic/` directory for gRPC client implementation patterns. |