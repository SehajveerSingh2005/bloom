# Contributing to Bloom

Thanks for wanting to help. Bloom is a Windows desktop companion built with Tauri — a Rust core and a React/TypeScript UI.

## Before you start

- Bugs and feature ideas go through the issue templates. They keep triage automatic.
- For anything large (new features, refactors, new dependencies), open an issue first so we can agree on the approach before you spend time on code.

## Setup

Requirements:

- Windows 10 or 11
- [Bun](https://bun.sh) 1.4+
- Rust stable
- Visual Studio C++ build tools (required by Tauri)

```bash
bun install
bun run tauri dev
```

Useful commands:

| Command               | What it does                      |
| --------------------- | --------------------------------- |
| `bun run build`       | Typecheck and build the frontend  |
| `bun run tauri build` | Build the full app                |
| `bun run bump <ver>`  | Bump the version in all manifests |

## Making changes

- Branch from `main` and keep each pull request focused on one change.
- Match the existing code style. Do not add dependencies unless they are really needed.
- Commit messages follow conventional commits (`feat:`, `fix:`, `chore:`), because release notes are generated from them.
- Test your change on Windows before opening the PR — this is a Windows-first app.

## Pull requests

- CI must pass (frontend build, `cargo check`, clippy, CodeQL).
- Fill in the PR template: what changed, which issue it closes, and how you tested it.
- Keep the diff readable. Unrelated cleanups belong in a separate PR.

## License

Bloom is licensed under GPL-3.0. By contributing, you agree that your contributions are licensed under the same terms.
