# React + TypeScript + Vite

This template provides a minimal setup to get React working in Vite with HMR and some ESLint rules.

Currently, two official plugins are available:

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react) uses [Oxc](https://oxc.rs)
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react-swc) uses [SWC](https://swc.rs/)

## React Compiler

The React Compiler is not enabled on this template because of its impact on dev & build performances. To add it, see [this documentation](https://react.dev/learn/react-compiler/installation).

## Expanding the ESLint configuration

If you are developing a production application, we recommend updating the configuration to enable type-aware lint rules:

```js
export default defineConfig([
	globalIgnores(["dist"]),
	{
		files: ["**/*.{ts,tsx}"],
		extends: [
			// Other configs...

			// Remove tseslint.configs.recommended and replace with this
			tseslint.configs.recommendedTypeChecked,
			// Alternatively, use this for stricter rules
			tseslint.configs.strictTypeChecked,
			// Optionally, add this for stylistic rules
			tseslint.configs.stylisticTypeChecked

			// Other configs...
		],
		languageOptions: {
			parserOptions: {
				project: ["./tsconfig.node.json", "./tsconfig.app.json"],
				tsconfigRootDir: import.meta.dirname
			}
			// other options...
		}
	}
]);
```

You can also install [eslint-plugin-react-x](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-x) and [eslint-plugin-react-dom](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-dom) for React-specific lint rules:

```js
// eslint.config.js
import reactX from "eslint-plugin-react-x";
import reactDom from "eslint-plugin-react-dom";

export default defineConfig([
	globalIgnores(["dist"]),
	{
		files: ["**/*.{ts,tsx}"],
		extends: [
			// Other configs...
			// Enable lint rules for React
			reactX.configs["recommended-typescript"],
			// Enable lint rules for React DOM
			reactDom.configs.recommended
		],
		languageOptions: {
			parserOptions: {
				project: ["./tsconfig.node.json", "./tsconfig.app.json"],
				tsconfigRootDir: import.meta.dirname
			}
			// other options...
		}
	}
]);
```

## Announcements

`public/announcements.json` is polled once per day by the Bloom app (3.9.1+) and shown as a persistent notch card and a Settings banner until dismissed. It is served at `https://bloom.sehaz.space/announcements.json`.

Publish one by replacing the contents:

```json
{
  "id": "2026-09-26-updater-manual-update",
  "severity": "warning",
  "title": "Update checks may fail on some networks",
  "body": "Bloom 3.8.7 and 3.9.0 can time out reaching GitHub. If no update is found, install the latest release manually.",
  "url": "https://github.com/SehajveerSingh2005/bloom/releases/latest"
}
```

- `id` and `title` are required; `body` and `url` are optional. `{}` means no announcement.
- `id` must change for each new announcement — dismissals are per id, and a new id re-opens the card.
- `severity` is `info` (default) or `warning` (orange accent).
- Changes go live with the normal website deploy; running apps pick it up within an hour (or on their next launch).
