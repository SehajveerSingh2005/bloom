import { expect, test } from "bun:test";
import { escapeLoneDollars, plainText, safeHref } from "../src/ai/mdText";

test("links open only for http, https and mailto", () => {
	expect(safeHref("https://example.com/a?b=1")).toBe("https://example.com/a?b=1");
	expect(safeHref("http://example.com")).toBe("http://example.com/");
	expect(safeHref("mailto:a@b.com")).toBe("mailto:a@b.com");
	expect(safeHref("javascript:alert(1)")).toBeNull();
	expect(safeHref(" JavaScript:alert(1)")).toBeNull();
	expect(safeHref("file:///C:/Windows/System32/calc.exe")).toBeNull();
	expect(safeHref("data:text/html,<b>x</b>")).toBeNull();
	expect(safeHref("ms-settings:display")).toBeNull();
	expect(safeHref("/relative/path")).toBeNull();
	expect(safeHref("")).toBeNull();
	expect(safeHref(undefined)).toBeNull();
});

test("the caption drops Markdown syntax but keeps the words", () => {
	expect(plainText("**Done.** Saved `notes.txt` to _Downloads_.")).toBe(
		"Done. Saved notes.txt to Downloads."
	);
	expect(plainText("# Title\n- one\n- two\n> quoted")).toBe("Title one two quoted");
	expect(plainText("See [the docs](https://x.y) now")).toBe("See the docs now");
	expect(plainText("Area is $\\pi r^2$ and $$E=mc^2$$")).toBe("Area is \\pi r^2 and E=mc^2");
	expect(plainText("It costs $5 to $10 a month")).toBe("It costs $5 to $10 a month");
	expect(plainText("| a | b |\n|---|---|\n| 1 | 2 |")).toBe("a b 1 2");
	expect(plainText("snake_case_name stays")).toBe("snake_case_name stays");
});

test("lone dollars are escaped so money is not math", () => {
	const keeps = (s: string) => expect(escapeLoneDollars(s)).toBe(s);
	expect(escapeLoneDollars("It costs $5 to $10 a month.")).toBe(
		String.raw`It costs \$5 to \$10 a month.`
	);
	keeps("Area is $x^2$ here");
	keeps(String.raw`$$\int_0^1 x\,dx$$`);
	keeps(String.raw`$$\int$$`);
	keeps("Run `$5` or `echo $HOME`");
	keeps("``a `$5` b``");
	expect(escapeLoneDollars("```sh\necho $PATH $1\n```\nthen $5")).toBe(
		"```sh\necho $PATH $1\n```\nthen \\$5"
	);
	expect(escapeLoneDollars("between $x$5 and $y$")).toBe(String.raw`between \$x\$5 and $y$`);
	expect(escapeLoneDollars(String.raw`$ 5 and \$3`)).toBe(String.raw`\$ 5 and \$3`);
});
