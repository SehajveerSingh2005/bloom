import { expect, test } from "bun:test";
import { ACTIVATE_S, OrbMotion, TARGETS, orbStateFor, type OrbParam } from "../src/ai/orbMotion";

const FRAME = 1 / 60;
// The most any drawn value may move in one 60 Hz frame. A snapped target
// (the old CSS) moves 0.3 to 1.0 in a single frame.
const MAX_STEP: Partial<Record<OrbParam, number>> = { opacity: 0.1, scale: 0.08, glow: 0.1, shells: 0.1, core: 0.1, tint: 0.1, spread: 0.02, tilt: 2 };

/** Runs frames and fails on any jump between consecutive frames. */
function run(m: OrbMotion, seconds: number) {
	for (let t = 0; t < seconds; t += FRAME) {
		const before = { ...m.x, spin: m.spinAngle };
		m.step(FRAME);
		for (const [p, max] of Object.entries(MAX_STEP)) {
			const d = Math.abs(m.x[p as OrbParam] - before[p as OrbParam]);
			if (d > max!) throw new Error(`${p} jumped ${d.toFixed(3)} in one frame (state ${m.state})`);
		}
		// Rotation only moves at the current (spring-driven) speed.
		expect(m.spinAngle - before.spin).toBeLessThanOrEqual(5 * FRAME);
	}
}

const near = (m: OrbMotion, p: OrbParam, value: number, tol = 0.02) =>
	expect(Math.abs(m.x[p] - value)).toBeLessThan(tol);

test("phases map to orb states", () => {
	expect(orbStateFor("recording", true, true)).toBe("listening");
	expect(orbStateFor("transcribing", true, true)).toBe("thinking");
	expect(orbStateFor("working", true, true)).toBe("thinking");
	expect(orbStateFor("done", true, true)).toBe("speaking");
	expect(orbStateFor("done", true, false)).toBe("rest");
	expect(orbStateFor("error", true, true)).toBe("error");
	expect(orbStateFor("idle", true, true)).toBe("idle");
	expect(orbStateFor("idle", true, false)).toBe("rest");
	expect(orbStateFor("working", false, true)).toBe("idle");
});

test("idle to listening blooms through activating, then listens", () => {
	const m = new OrbMotion("idle");
	expect(m.frame().opacity).toBe(0);
	near(m, "scale", 0.6);
	m.setState("listening");
	expect(m.state).toBe("activating");
	run(m, ACTIVATE_S - 0.05);
	expect(m.state).toBe("activating");
	run(m, 0.1);
	expect(m.state).toBe("listening");
	run(m, 1.5);
	near(m, "opacity", 1);
	near(m, "glow", TARGETS.listening.glow);
	expect(m.settled()).toBe(false); // it keeps pulsing
});

test("scale overshoots a little on the way in", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	let peak = 0;
	for (let t = 0; t < 1.5; t += FRAME) {
		m.step(FRAME);
		peak = Math.max(peak, m.x.scale);
	}
	expect(peak).toBeGreaterThan(TARGETS.activating.scale);
	expect(peak).toBeLessThan(TARGETS.activating.scale * 1.1);
});

test("listening, thinking, speaking, idle: one continuous motion", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	run(m, 1);
	m.setState("thinking");
	run(m, 1.5);
	near(m, "spread", TARGETS.thinking.spread, 0.01);
	m.setState("speaking");
	run(m, 1.5);
	near(m, "core", TARGETS.speaking.core);
	m.setState("idle");
	expect(m.state).toBe("settling");
	run(m, 2);
	expect(m.state).toBe("idle");
	expect(m.settled()).toBe(true);
});

test("rapid listening, thinking, speaking stays continuous", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	run(m, 0.5);
	for (const s of ["thinking", "speaking", "listening", "thinking", "speaking"] as const) {
		m.setState(s);
		run(m, 0.05);
	}
	expect(m.state).toBe("speaking");
	run(m, 2);
	near(m, "glow", TARGETS.speaking.glow);
});

test("interrupting a fade-out picks up from where it is", () => {
	const m = new OrbMotion("idle");
	m.setState("speaking");
	run(m, 1.5);
	m.setState("idle");
	run(m, 0.1);
	const mid = m.x.opacity;
	expect(mid).toBeGreaterThan(0.2);
	expect(mid).toBeLessThan(0.98);
	m.setState("listening"); // a new "Hey Janice" mid-fade
	expect(m.state).toBe("activating");
	run(m, 0.2);
	expect(m.x.opacity).toBeGreaterThan(mid - 0.1);
	run(m, 1);
	expect(m.state).toBe("listening");
});

test("a state change mid-entry keeps the burst and lands on the latest", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	run(m, 0.1);
	m.setState("thinking");
	expect(m.state).toBe("activating");
	run(m, 0.5);
	expect(m.state).toBe("thinking");
});

test("error tints red smoothly and comes to rest", () => {
	const m = new OrbMotion("idle");
	m.setState("thinking");
	run(m, 1);
	m.setState("error");
	run(m, 0.05);
	expect(m.x.tint).toBeGreaterThan(0);
	expect(m.x.tint).toBeLessThan(0.3);
	run(m, 3);
	near(m, "tint", 1);
	expect(m.settled()).toBe(true); // still: the frame loop can stop
});

test("settle-then-unmount: settled only once invisible, and it stays put", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	run(m, 1);
	m.setState("idle");
	let frames = 0;
	while (!m.settled()) {
		expect(frames++).toBeLessThan(120); // gone within 2 s
		m.step(FRAME);
		if (!m.settled()) expect(m.x.opacity).toBeGreaterThan(0);
	}
	expect(m.frame().opacity).toBeLessThan(0.002);
	// Parked: the next show starts from the idle shape.
	near(m, "scale", TARGETS.idle.scale, 1e-9);
	expect(m.v.opacity).toBe(0);
});

test("the panel's orb rests and stops", () => {
	const m = new OrbMotion("rest");
	expect(m.settled()).toBe(true);
	m.setState("thinking");
	run(m, 1);
	expect(m.settled()).toBe(false);
	m.setState("rest");
	run(m, 4);
	expect(m.settled()).toBe(true);
});

test("reduced motion: no scale, spin or drift, only fades", () => {
	const m = new OrbMotion("idle", true);
	expect(m.x.scale).toBe(1);
	m.setState("thinking");
	expect(m.state).toBe("thinking"); // no entry burst
	run(m, 1);
	expect(m.frame().scale).toBe(1);
	expect(m.spinAngle).toBe(0);
	expect(m.x.spread).toBe(0);
	near(m, "opacity", TARGETS.thinking.opacity);
	expect(m.settled()).toBe(true);
	m.setState("idle");
	run(m, 2);
	expect(m.settled()).toBe(true);
	expect(m.frame().opacity).toBeLessThan(0.002);
});

test("a long frame gap does not teleport", () => {
	const m = new OrbMotion("idle");
	m.setState("listening");
	m.step(2); // the window was hidden for two seconds
	expect(m.x.opacity).toBeLessThan(0.5);
});
