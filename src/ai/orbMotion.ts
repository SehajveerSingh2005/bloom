// Janice's orb as one physical system (AiOrb.tsx draws it). Every visual
// parameter is a damped spring chasing its state's target, so any state can
// interrupt any other mid-flight without a jump or a restart. Rotation and
// breathing are phase angles integrated from spring-driven speeds, so a speed
// change bends the motion instead of snapping it. Pure, so it is tested with
// `bun test scripts/orb-motion.test.ts`.

import type { AiPhase } from "./aiState";

/** `idle` is hidden; `rest` is the panel's still orb; `settling` is idle until it has faded out. */
export type OrbState = "idle" | "rest" | "activating" | "listening" | "thinking" | "speaking" | "error" | "settling";
type Target = Exclude<OrbState, "settling">;

const PARAMS = ["opacity", "scale", "glow", "shells", "core", "tint", "spin", "breathe", "breatheRate", "spread", "tilt"] as const;
export type OrbParam = (typeof PARAMS)[number];
type Values = Record<OrbParam, number>;

// [stiffness, damping ratio] with unit mass. Below 1 overshoots a little.
const SPRING: Record<OrbParam, [number, number]> = {
	opacity: [90, 1],
	scale: [170, 0.62],
	glow: [60, 1],
	shells: [80, 1],
	core: [80, 1],
	tint: [40, 1],
	spin: [12, 1], // rad/s of the shells' rotation
	breathe: [20, 1], // breathing amplitude, a fraction of the size
	breatheRate: [10, 1], // rad/s of the breathing phase
	spread: [40, 0.8], // how far the shells drift off centre, a fraction of the size
	tilt: [30, 0.85] // extra shell tilt, degrees
};

const LIVE = { tint: 0 };
export const TARGETS: Record<Target, Values> = {
	idle: { opacity: 0, scale: 0.6, glow: 0, shells: 0, core: 0, tint: 0, spin: 0, breathe: 0, breatheRate: 1, spread: 0, tilt: 0 },
	rest: { opacity: 0.5, scale: 1, glow: 0.35, shells: 0.8, core: 0.8, tint: 0, spin: 0, breathe: 0, breatheRate: 1, spread: 0, tilt: 0 },
	activating: { ...LIVE, opacity: 1, scale: 1.12, glow: 0.85, shells: 0.9, core: 1, spin: 3.2, breathe: 0.02, breatheRate: 5, spread: 0.04, tilt: 10 },
	// No mic level reaches the webview, so listening is a quick, gentle pulse.
	listening: { ...LIVE, opacity: 1, scale: 1, glow: 0.6, shells: 0.85, core: 0.85, spin: 2.4, breathe: 0.05, breatheRate: 5.7, spread: 0.03, tilt: 8 },
	thinking: { ...LIVE, opacity: 0.95, scale: 1.06, glow: 0.5, shells: 0.9, core: 0.7, spin: 4.2, breathe: 0.03, breatheRate: 3.5, spread: 0.08, tilt: 16 },
	speaking: { ...LIVE, opacity: 1, scale: 1.03, glow: 0.75, shells: 0.8, core: 1, spin: 1.2, breathe: 0.035, breatheRate: 2.4, spread: 0.02, tilt: 4 },
	error: { opacity: 0.92, scale: 0.97, glow: 0.55, shells: 0.7, core: 0.6, tint: 1, spin: 0, breathe: 0, breatheRate: 1, spread: 0, tilt: 0 }
};

/** Motion parameters: reduced motion holds them still, so only fades remain. */
const MOVING: OrbParam[] = ["scale", "spin", "breathe", "breatheRate", "spread", "tilt"];
const STILL: Partial<Values> = { scale: 1, spin: 0, breathe: 0, spread: 0, tilt: 0 };
const LIVE_STATES: Target[] = ["listening", "thinking", "speaking"];
/** How long the entry burst lasts before the requested state takes over. */
export const ACTIVATE_S = 0.28;
const SUBSTEP = 1 / 240;
const MAX_DT = 0.05;

/**
 * The orb's state for a request phase. `float` is the overlay's orb, which
 * hides when idle or not shown and keeps flowing while the answer is up; the
 * panel's inline orb rests instead so it costs nothing while it waits.
 */
export function orbStateFor(phase: AiPhase, shown: boolean, float: boolean): Target {
	if (!shown) return "idle";
	switch (phase) {
		case "recording":
			return "listening";
		case "transcribing":
		case "working":
			return "thinking";
		case "confirm":
			return "speaking";
		case "done":
			return float ? "speaking" : "rest";
		case "error":
			return "error";
		default:
			return float ? "idle" : "rest";
	}
}

export class OrbMotion {
	readonly x: Values;
	readonly v: Values;
	/** Accumulated phase angles (radians): rotation and breathing never restart. */
	spinAngle = 0;
	breathAngle = 0;
	private current: OrbState;
	private pending: Target = "listening";
	private hold = 0;
	private reduced: boolean;

	constructor(initial: Target = "idle", reduced = false) {
		this.reduced = reduced;
		this.current = initial;
		this.x = { ...this.goal() };
		this.v = Object.fromEntries(PARAMS.map((p) => [p, 0])) as Values;
	}

	get state(): OrbState {
		return this.current;
	}

	setReduced(reduced: boolean) {
		this.reduced = reduced;
	}

	setState(next: Target) {
		const from = this.current;
		if (next === "idle") {
			if (from !== "idle") this.current = "settling";
			return;
		}
		if (LIVE_STATES.includes(next) && !this.reduced) {
			if (from === "activating") {
				this.pending = next;
				return;
			}
			if (from === "idle" || from === "settling" || from === "rest") {
				this.current = "activating";
				this.pending = next;
				this.hold = ACTIVATE_S;
				return;
			}
		}
		this.current = next;
	}

	/** The targets the springs chase right now. */
	goal(): Values {
		const key: Target = this.current === "settling" ? "idle" : this.current;
		return this.reduced ? { ...TARGETS[key], ...STILL } : TARGETS[key];
	}

	step(dt: number) {
		dt = Math.min(Math.max(dt, 0), MAX_DT);
		if (this.current === "activating") {
			this.hold -= dt;
			if (this.hold <= 0) this.current = this.pending;
		}
		const goal = this.goal();
		// Reduced motion: parked while invisible; if it turns on mid-flight, the
		// springs carry the motion to a stop instead of snapping it.
		if (this.reduced && this.x.opacity < 0.002) for (const p of MOVING) (this.x[p] = goal[p]), (this.v[p] = 0);
		for (let left = dt; left > 1e-9; left -= SUBSTEP) {
			const h = Math.min(SUBSTEP, left);
			for (const p of PARAMS) {
				const [k, zeta] = SPRING[p];
				// Semi-implicit Euler: stable for these stiffnesses at 240 Hz.
				this.v[p] += (-k * (this.x[p] - goal[p]) - 2 * zeta * Math.sqrt(k) * this.v[p]) * h;
				this.x[p] += this.v[p] * h;
			}
			this.spinAngle += this.x.spin * h;
			this.breathAngle += this.x.breatheRate * h;
		}
		if (this.current === "settling" && this.hiddenAndStill()) this.snapIdle();
	}

	private hiddenAndStill(): boolean {
		return this.x.opacity < 0.002 && Math.abs(this.v.opacity) < 0.02;
	}

	/** Invisible now: park every spring on idle so the next show starts clean. */
	private snapIdle() {
		this.current = "idle";
		Object.assign(this.x, this.goal());
		for (const p of PARAMS) this.v[p] = 0;
	}

	/**
	 * True when nothing would change on screen: hidden, or every spring is at
	 * rest on a still state. The renderer stops its frame loop then.
	 */
	settled(): boolean {
		if (this.current === "idle") return true;
		if (this.current === "settling" || this.current === "activating") return false;
		const goal = this.goal();
		if (goal.spin !== 0 || goal.breathe !== 0) return false;
		// The breathing rate is invisible once the amplitude is 0.
		return PARAMS.every(
			(p) => p === "breatheRate" || (Math.abs(this.x[p] - goal[p]) < 5e-3 && Math.abs(this.v[p]) < 2e-2)
		);
	}

	/** Values to draw: clamped fades and the breathing folded into the scale. */
	frame() {
		const x = this.x;
		const clamp = (n: number) => Math.min(1, Math.max(0, n));
		return {
			opacity: clamp(x.opacity),
			scale: Math.max(0, x.scale * (1 + x.breathe * Math.sin(this.breathAngle))),
			glow: clamp(x.glow),
			shells: clamp(x.shells),
			core: clamp(x.core),
			tint: clamp(x.tint),
			spin: this.spinAngle,
			spread: x.spread,
			tilt: x.tilt
		};
	}
}
