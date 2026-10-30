import { expect, test } from "bun:test";
import { waitForPendingSaves } from "../src/settings/pendingSaves";

test("closing waits for a delayed save", async () => {
	let finish!: () => void;
	const pending = new Promise<void>((resolve) => { finish = resolve; });
	let closed = false;
	const closing = waitForPendingSaves(() => pending).then(() => { closed = true; });
	await Promise.resolve();
	expect(closed).toBe(false);
	finish();
	await closing;
	expect(closed).toBe(true);
});

test("closing includes follow-up writes queued after an earlier save", async () => {
	let finish!: () => void;
	const first = new Promise<void>((resolve) => { finish = resolve; });
	let pending = first;
	const writes: number[] = [];
	void first.then(() => {
		pending = Promise.resolve().then(() => {
			writes.push(1);
			pending = Promise.resolve().then(() => { writes.push(2); });
		});
	});
	const closing = waitForPendingSaves(() => pending);
	finish();
	await closing;
	expect(writes).toEqual([1, 2]);
});

test("a rejected save prevents close from proceeding", async () => {
	const pending = Promise.reject(new Error("save failed"));
	await expect(waitForPendingSaves(() => pending)).rejects.toThrow("save failed");
});
