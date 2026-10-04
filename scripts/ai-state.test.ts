import { expect, test } from "bun:test";
import { IDLE, reduceAiEvent } from "../src/ai/aiState";
import { cleanAiName } from "../src/ai/aiName";

test("a voice request: recording, transcribing, working, done", () => {
	let s = reduceAiEvent(IDLE, { type: "recording", on: true });
	expect(s.phase).toBe("recording");
	s = reduceAiEvent(s, { type: "recording", on: false });
	expect(s.phase).toBe("transcribing");
	s = reduceAiEvent(s, { type: "transcript", task: 1, text: "make a grocery list" });
	expect(s).toMatchObject({ phase: "working", heard: "make a grocery list" });
	s = reduceAiEvent(s, { type: "activity", task: 1, text: "Writing groceries.txt" });
	expect(s.activity).toBe("Writing groceries.txt");
	s = reduceAiEvent(s, { type: "reply", task: 1, text: "Saved it to Downloads." });
	expect(s).toMatchObject({ phase: "done", reply: "Saved it to Downloads.", activity: "" });
});

test("a confirm card shows, then clears on the reply", () => {
	let s = reduceAiEvent({ ...IDLE, phase: "working" }, {
		type: "confirm", task: 2, id: 9, kind: "email", title: "Send this to neha@example.com?", body: "Subject: Soccer"
	});
	expect(s.phase).toBe("confirm");
	expect(s.confirm).toEqual({ id: 9, kind: "email", title: "Send this to neha@example.com?", body: "Subject: Soccer" });
	s = reduceAiEvent(s, { type: "reply", task: 2, text: "Sent." });
	expect(s.confirm).toBeNull();
});

test("task-less errors (from Settings) leave an idle panel alone", () => {
	expect(reduceAiEvent(IDLE, { type: "error", task: null, message: "x" })).toBe(IDLE);
	expect(reduceAiEvent(IDLE, { type: "error", task: 0, message: "AI is off" }).phase).toBe("error");
});

test("the agent exiting mid-request is an error; when idle it is not", () => {
	expect(reduceAiEvent({ ...IDLE, phase: "working" }, { type: "exited" }).phase).toBe("error");
	expect(reduceAiEvent(IDLE, { type: "exited" })).toBe(IDLE);
});

test("a stray recording-off does not leave a finished request", () => {
	const done = { ...IDLE, phase: "done" as const, reply: "ok" };
	expect(reduceAiEvent(done, { type: "recording", on: false })).toBe(done);
});

test("ignore stale activity event after reply", () => {
	const done = { ...IDLE, phase: "done" as const, reply: "Saved." };
	expect(reduceAiEvent(done, { type: "activity", task: 1, text: "Writing..." })).toBe(done);
});

test("an old task's error is ignored after a newer task starts; the current reply lands", () => {
	let s = reduceAiEvent(IDLE, { type: "transcript", task: 2, text: "new" });
	expect(reduceAiEvent(s, { type: "error", task: 1, message: "Stopped." })).toBe(s);
	expect(reduceAiEvent(s, { type: "reply", task: 1, text: "old" })).toBe(s);
	s = reduceAiEvent(s, { type: "reply", task: 2, text: "ok" });
	expect(s).toMatchObject({ phase: "done", reply: "ok" });
});

test("the assistant's name is validated", () => {
	expect(cleanAiName("  Mina ")).toBe("Mina");
	expect(cleanAiName("Anne-Marie O'Neil")).toBe("Anne-Marie O'Neil");
	for (const bad of ["", "  ", "R2D2", "a<b", "x".repeat(25), null, undefined]) expect(cleanAiName(bad)).toBe("Janice");
});
