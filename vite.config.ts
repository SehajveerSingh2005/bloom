import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import svgr from "vite-plugin-svgr";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// KaTeX's CSS lists each font as woff2, woff and ttf. WebView2 only ever
// loads woff2, so the other two (about 800 kB) stay out of the bundle.
const katexWoff2Only = {
	name: "katex-woff2-only",
	enforce: "pre" as const,
	transform(code: string, id: string) {
		if (!id.includes("katex") || !id.endsWith(".css")) return;
		return code.replace(
			/,url\([^)]+\.woff\) format\("woff"\),url\([^)]+\.ttf\) format\("truetype"\)/g,
			""
		);
	}
};

// https://vite.dev/config/
export default defineConfig(async () => ({
	plugins: [react(), svgr(), katexWoff2Only],

	// Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
	//
	// 1. prevent Vite from obscuring rust errors
	clearScreen: false,
	// 2. tauri expects a fixed port, fail if that port is not available
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host
			? {
					protocol: "ws",
					host,
					port: 1421
				}
			: undefined,
		watch: {
			// 3. tell Vite to ignore watching `src-tauri`
			ignored: ["**/src-tauri/**"]
		}
	},

	// Multiple HTML entry points
	build: {
		rollupOptions: {
			input: {
				main: "index.html",
				overlay: "overlay.html",
				settings: "settings.html",
				dock: "dock.html"
			}
		}
	}
}));
