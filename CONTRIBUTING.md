# Contributing to Bloom

Bloom is a lightweight Windows desktop companion: a dock, a notch, and a small set of system controls. Contributions should make those surfaces more reliable, useful, and consistent.

## Pick the shortest path

- **Small bug fix, documentation, or typo:** a focused PR is welcome directly. Explain the problem and how you checked it. An issue is helpful, not mandatory.
- **New feature, larger behavior change, refactor, or dependency:** open one focused issue first and wait for a maintainer to agree on the scope and approach. A draft or prototype can help discussion, but is not a promise of review or acceptance.
- **Already reported:** add reproduction details or offer to help on the existing issue instead of opening another.
- A bug report needs no prior approval. Reports against the latest release are welcome even when a fix may already be on main; mention which version you tested.

An issue being open, a bot label, passing CI, or another contributor agreeing is not feature approval. Maintainers use the `accepted` label after agreeing on an implementation scope. Earlier explicit maintainer agreement still counts; ask for the label if an advisory misses it.

## Scope and review capacity

Prioritize shell correctness, input/focus behavior, accessibility, and low idle CPU/memory use. Keep the dock for apps and the notch for compact information and controls. Native Windows fallbacks are preferable to duplicating whole applications.

A replacement Start menu, embedded AI assistant, desktop widget framework, arbitrary CSS/plugin system, or additional operating-system port needs a separate maintainer-backed plan; these are not part of the current roadmap.

Please discuss one substantial proposal at a time and wait for feedback before opening a batch of feature PRs. There is no numerical PR quota, and independent bug fixes are welcome. Keep speculative work in your fork or a draft while scope is discussed.

## Issues

Use the bug or feature form and search both open and closed issues first.

- Bugs: Bloom/Windows versions, expected versus actual behavior, and a short reproduction. Screenshots, logs, or a recording help when available; they are not a barrier to reporting.
- Features: one user problem, a small proposed change, and how it fits the existing UI.
- `needs-discussion`: scope or approach is not yet agreed. This is not a rejection.
- `accepted`: a maintainer has agreed on the scope; it is not a delivery date or permission to expand the feature.
- `needs-info` / `needs-testing`: the next useful step is more detail or validation.
- Completed issues close with a fix/reference. Duplicates link to the canonical thread. Not-planned proposals close with a reason and can be revisited if the direction changes.
- Bugs are not automatically closed for inactivity. A missing local reproduction is not proof that a bug is fixed.

Automated intake is advisory: it may label a feature PR and leave one updated reminder, but it does not close work, approve features, or add a required merge gate. Editing the PR description or pushing an update refreshes the reminder.

## Setup

Windows 10 or 11, Bun 1.4+, Rust stable, and Visual Studio C++ build tools are required.

```bash
bun install
bun run tauri dev
```

Do not run a separate Vite dev server alongside Tauri.

| Command | Purpose |
| --- | --- |
| `bun run build` | Frontend typecheck and production build |
| `cargo check --locked` in `src-tauri` | Rust compile check |
| `cargo clippy --locked --all-targets` in `src-tauri` | Rust diagnostics |
| `cargo test --locked` in `src-tauri` | Rust unit tests (not run by CI) |
| `bun run tauri build` | Build the app |
| `bun run format` | Format frontend and Rust |

## Pull requests

Branch from current main and keep each PR focused. Link an issue when one exists; use `Closes #123` only when the whole issue is resolved. Describe partial work as `Related to #123`.

State what you actually tested. Windows behavior changes need an appropriate Windows reproduction/check before merging; a cross-platform build or unit test alone does not verify window focus, taskbar restoration, tray actions, or DPI behavior. If you cannot test locally, say so and leave the PR as a draft or request help. Documentation changes do not need desktop testing.

CI must pass before merge. A passing build does not establish product fit or replace review. Match the code style, avoid unnecessary dependencies, and use conventional commit messages (`feat:`, `fix:`, `chore:`) for release notes. Keep unrelated cleanups separate.

Feature PRs without agreed scope may be closed with a pointer back to the issue; they can be reopened or replaced after discussion. We review the change and its fit, not the contributor's PR count.

## License

Bloom is GPL-3.0. Contributions are licensed under the same terms.
