import { expect, test } from "bun:test";
import { SETTLE_MS, acceptsReading } from "../src/hooks/useSliderSync";

test("ignores readings while the slider is being dragged", () => {
	const now = 10_000;
	expect(acceptsReading(now, now)).toBe(false);
	expect(acceptsReading(now - 200, now)).toBe(false);
});

test("ignores readings during the settle window after the last input", () => {
	const now = 10_000;
	expect(acceptsReading(now - (SETTLE_MS - 1), now)).toBe(false);
});

test("accepts external changes once the slider has settled", () => {
	const now = 10_000;
	expect(acceptsReading(now - SETTLE_MS, now)).toBe(true);
	expect(acceptsReading(0, now)).toBe(true);
});
