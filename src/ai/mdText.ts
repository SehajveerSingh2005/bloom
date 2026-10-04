// Pure Markdown helpers for Janice's replies, tested with `bun test scripts/ai-markdown.test.ts`.

const LINK_PROTOCOLS = ["http:", "https:", "mailto:"];

/** A link the model wrote, if it may open in the default browser or mail app; null renders it as text. */
export function safeHref(href: unknown): string | null {
	if (typeof href !== "string") return null;
	try {
		const url = new URL(href.trim());
		return LINK_PROTOCOLS.includes(url.protocol) ? url.href : null;
	} catch {
		return null;
	}
}

/**
 * A reply as plain text for the orb's two-line caption: the Markdown syntax
 * (and math delimiters) goes, the words stay.
 * ponytail: regexes, not a parser; enough for a clamped caption.
 */
export function plainText(md: string): string {
	return md
		.replace(/```[^\n]*\n?/g, "")
		.replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
		.replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
		.replace(/^\s{0,3}(#{1,6}\s+|>\s?|[-*+]\s+(\[[ xX]\]\s+)?|\d+[.)]\s+)/gm, "")
		.replace(/^\s*\|?(\s*:?-{3,}:?\s*\|?)+\s*$/gm, "")
		.replace(/\s*\|\s*/g, " ")
		.replace(/\$\$|\\\(|\\\)|\\\[|\\\]/g, "")
		.replace(/\$(\S(?:[^$]*\S)?)\$/g, "$1")
		.replace(/(\*\*|__|~~|\*|`)/g, "")
		.replace(/(^|\W)_(\S[^_]*\S|\S)_(?=\W|$)/g, "$1$2")
		.replace(/\s+/g, " ")
		.trim();
}
