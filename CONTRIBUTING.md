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

| Command               | What it does                          |
| --------------------- | ------------------------------------- |
| `bun run build`       | Typecheck and build the frontend      |
| `bun run tauri build` | Build the full app                    |
| `bun run bump <ver>`  | Bump the version in all manifests     |
| `bun run format`      | Formats the frontend and backend code |

## Making changes

- Branch from `main` and keep each pull request focused on one change.
- Match the existing code style. Do not add dependencies unless they are really needed.
- Commit messages follow conventional commits (`feat:`, `fix:`, `chore:`), because release notes are generated from them.
- Test your change on Windows before opening the PR — this is a Windows-first app.

## Translations

Bloom ships a small dependency-free i18n layer in `src/i18n/`. English (`src/i18n/locales/en.json`) is the source of truth. Other locales may be partial: a missing key falls back to English at runtime, so translations can catch up at their own pace after a feature ships.

Adding or improving a language:

1. Copy `src/i18n/locales/en.json` to `<code>.json` (for example `de.json`).
2. Translate the values. Keep `{placeholders}` exactly as they are, and use the plural group that fits the language (`one` / `few` / `many` / `other`). You can start with only the sections you know — partial files are valid.
3. Register the locale in the `LOCALES` list in `src/i18n/index.ts`.
4. Run `bun run i18n:check`. It prints coverage per locale and lists any missing keys; invalid entries (unknown keys, placeholder mismatches, empty values) fail the check.
5. Run `bun run build` and try it in Settings → General → Language. Language changes apply live; no restart is needed.

Notes:

- Do not translate brand names (Bloom, Windows), system data (app names, window titles, city names), setting keys, or symbol-only strings.
- Use `tCount("key.base", count)` for counts. Store the group as `key.base.one`, `key.base.few`, etc.
- UI strings should stay short — they render in a small notch, dock and popovers.
- Translating a new string later? Run `bun run i18n:check` — it lists exactly which keys are missing.

## Pull requests

- CI must pass (frontend build, `cargo check`, clippy, CodeQL).
- Fill in the PR template: what changed, which issue it closes, and how you tested it.
- Keep the diff readable. Unrelated cleanups belong in a separate PR.

## License

Bloom is licensed under GPL-3.0. By contributing, you agree that your contributions are licensed under the same terms.
