# Janice Round 5: Panel Polish

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Fix clipping in Janice's panel and the Settings Test button, add a thinking animation, render full Markdown (GFM tables, code, LaTeX math) in replies, and make the answer pill expand, appear and disappear smoothly.

## Global Constraints
- All earlier constraints hold (no em dashes; never touch Credential Manager except via Delete AI; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution; cohesion with the notch command centre and dock info panel first; animations only while something is happening; respect `prefers-reduced-motion`).
- After this round the live `bloom/` folder must be re-synced from the branch (controller task) so `update.bat` keeps working.

## Tasks

### Task 1: Clipping, thinking animation, Markdown, smooth answer pill
User report: "the output/input from janice in window is clipped, also test btn is clipped. it should have a cool thinking animation when its doing smt. it should be able to render full md including tables and equations. her answer pill can expand cleanly and should appear and disappear cleanly not suddenly."
- Clipping: find every place Janice's text, input row or buttons are cut off in the notch panel (`src/App.tsx` fixed per-phase heights, `.ai-notch-content`), the dock Janice tab (`src/InfoCentre.tsx`, `.ic-panel` fixed 300px, `src/Dock.css`), the wake orb caption (`src/Overlay.tsx`/`Overlay.css`), and the Settings AI tab (`src/settings/AiTab.tsx`/`AiTab.css`, e.g. the email Test button next to the app password field inside `SettingRow`). Fix layouts so nothing important is ever clipped: long replies scroll inside the body, the input row and action buttons are always fully visible, Settings rows wrap or size correctly.
- Notch height: replace guessed fixed heights with a measured content height (clamped to a sensible max, e.g. 360px) that the notch's spring animates to, so the pill grows/shrinks smoothly with its content instead of clipping or jumping.
- Thinking animation: while `transcribing`/`working` (and between tool steps) show a polished "thinking" indicator inside the panel that matches Bloom's style (e.g. the aura orb intensifying plus a shimmering "Thinking" label or animated dots/gradient sweep on the status line), CSS transform/opacity only, off when idle, reduced-motion = static.
- Markdown: render replies with GitHub-flavoured Markdown (headings, lists, emphasis, links that open in the default browser via the opener plugin, inline/code blocks with wrapping, tables that scroll horizontally if wide, blockquotes) and math (`$...$`, `$$...$$`) via KaTeX. Prefer `react-markdown` + `remark-gfm` + `remark-math` + `rehype-katex` (or an equivalent smaller stack; justify). No raw HTML from the model (no `rehype-raw`); sanitize. KaTeX CSS/fonts bundled locally (CSP allows only 'self' fonts and inline styles; check `src-tauri/tauri.conf.json`). Style tables/code to Bloom's theme in notch, dock and orb caption (caption may stay plain-text/2-line).
- Answer pill motion: replies appear with a smooth fade/slide and height animation, and disappear the same way (framer-motion `AnimatePresence` + layout/height animation, matching the command-centre spring); no sudden pop-in/out; the notch/dock size change animates together.
- Tests: `bun run build`, `bun test scripts/ai-state.test.ts`; add a small test for any pure helper (e.g. link/sanitize policy). Report bundle-size impact.
