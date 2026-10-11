// Janice's replies as GitHub-flavoured Markdown with KaTeX math. Loaded on
// demand (AiPanel imports it lazily), so the taskbar starts without it.
// No raw HTML: react-markdown escapes it (no rehype-raw), images show their
// alt text, and only http/https/mailto links open, in the default browser.
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import { openUrl } from "@tauri-apps/plugin-opener";
import "katex/dist/katex.min.css";
import { escapeLoneDollars, safeHref } from "./mdText";

const components: Components = {
	a: ({ href, children }) => {
		const url = safeHref(href);
		if (!url) return <>{children}</>;
		return (
			<a
				href={url}
				title={url}
				onClick={(e) => {
					e.preventDefault();
					openUrl(url).catch(() => {});
				}}
				// A middle click would otherwise open a WebView2 window.
				onAuxClick={(e) => e.preventDefault()}
			>
				{children}
			</a>
		);
	},
	img: ({ alt }) => <>{alt}</>,
	// Wide tables scroll sideways instead of widening the panel.
	table: ({ children }) => (
		<div className="ai-md-table">
			<table>{children}</table>
		</div>
	)
};

export default function Markdown({ text }: { text: string }) {
	return (
		<div className="ai-md">
			<ReactMarkdown
				remarkPlugins={[remarkGfm, remarkMath]}
				rehypePlugins={[
					[rehypeKatex, { throwOnError: false, strict: "ignore", maxSize: 20, maxExpand: 200 }]
				]}
				components={components}
			>
				{escapeLoneDollars(text)}
			</ReactMarkdown>
		</div>
	);
}
