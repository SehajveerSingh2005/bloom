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
		.replace(/\$(\S(?:[^$]*\S)?)\$(?!\d)/g, "$1")
		.replace(/(\*\*|__|~~|\*|`)/g, "")
		.replace(/(^|\W)_(\S[^_]*\S|\S)_(?=\W|$)/g, "$1$2")
		.replace(/\s+/g, " ")
		.trim();
}

// Fenced code, inline code spans and $$display$$ math pass through untouched.
const PROTECTED = /(```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|(`+)[\s\S]*?\2|\$\$[\s\S]*?\$\$)/;

/**
 * Escapes every `$` that does not open or close inline math by Pandoc's rule
 * (the opening `$` is followed by a non-space; the closing one follows a
 * non-space and is not followed by a digit), so "$5 to $10" stays money.
 * Pairs the same way as plainText's math regex.
 * ponytail: indented (4-space) code blocks are not recognised; fenced ones are.
 */
export function escapeLoneDollars(md: string): string {
	// split() with two capture groups yields [text, protected, backticks, text, ...].
	return md
		.split(PROTECTED)
		.map((part, i) => (i % 3 === 0 ? escapeText(part) : (part ?? "")))
		.filter((_, i) => i % 3 !== 2)
		.join("");
}

function escapeText(text: string): string {
	let out = "";
	let i = 0;
	while (i < text.length) {
		const c = text[i];
		if (c === "\\") {
			out += text.slice(i, i + 2);
			i += 2;
		} else if (c !== "$") {
			out += c;
			i++;
		} else {
			const close = /\S/.test(text[i + 1] ?? "") ? closingDollar(text, i + 1) : -1;
			if (close < 0) {
				out += "\\$";
				i++;
			} else {
				out += text.slice(i, close + 1);
				i = close + 1;
			}
		}
	}
	return out;
}

/** The `$` closing math opened just before `from`, or -1 if the next `$` cannot close it. */
function closingDollar(text: string, from: number): number {
	for (let j = from; j < text.length; j++) {
		if (text[j] === "\\") j++;
		else if (text[j] === "$")
			return /\S/.test(text[j - 1]) && !/\d/.test(text[j + 1] ?? "") ? j : -1;
	}
	return -1;
}
