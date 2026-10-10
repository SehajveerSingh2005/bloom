import { useEffect, useLayoutEffect, useRef, type CSSProperties } from "react";
import { useReducedMotion } from "framer-motion";
import type { AiPhase } from "./aiState";
import { OrbMotion, orbStateFor } from "./orbMotion";
import "./orb.css";

// Each shell's resting tilt and how fast it turns relative to the spin.
const SHELLS = [
	{ base: "rotateX(18deg)", axis: "rotateX", rate: 1 },
	{ base: "rotateY(58deg)", axis: "rotateX", rate: -0.77 },
	{ base: "rotate3d(1, 1, 0, 62deg)", axis: "rotateX", rate: 1.25 }
];
const DEG = 180 / Math.PI;

/**
 * Janice's orb, the same everywhere: small in the panel's status line, large
 * on screen for a "Hey <name>" request (`float`). Driven by orbMotion's
 * springs from one requestAnimationFrame loop that writes styles straight to
 * the DOM (no React render per frame) and stops once the orb is still or
 * hidden. A `float` orb fades out when `shown` goes false and then calls
 * `onHidden`, so its owner can unmount it only after it has settled.
 */
export function AiOrb({
	phase,
	size,
	float = false,
	shown = true,
	onHidden
}: {
	phase: AiPhase;
	size: number;
	float?: boolean;
	shown?: boolean;
	onHidden?: () => void;
}) {
	const reduced = useReducedMotion() ?? false;
	const target = orbStateFor(phase, shown, float);
	const root = useRef<HTMLSpanElement>(null);
	const glow = useRef<HTMLSpanElement>(null);
	const shells = useRef<(HTMLSpanElement | null)[]>([]);
	const core = useRef<HTMLSpanElement>(null);
	const tint = useRef<HTMLSpanElement>(null);
	const motion = useRef<OrbMotion | null>(null);
	motion.current ??= new OrbMotion(float ? "idle" : target, reduced);
	const raf = useRef(0);
	// The frame loop outlives renders: everything it reads goes through refs.
	const onHiddenRef = useRef(onHidden);
	onHiddenRef.current = onHidden;
	const sizeRef = useRef(size);
	sizeRef.current = size;

	const draw = () => {
		const f = motion.current!.frame();
		const r = root.current;
		if (!r) return;
		r.style.opacity = f.opacity.toFixed(4);
		r.style.transform = `scale(${f.scale.toFixed(4)})`;
		glow.current!.style.opacity = f.glow.toFixed(4);
		core.current!.style.opacity = f.core.toFixed(4);
		tint.current!.style.opacity = f.tint.toFixed(4);
		const spread = f.spread * sizeRef.current;
		SHELLS.forEach((s, i) => {
			const el = shells.current[i]!;
			const a = f.spin * s.rate;
			// Each shell drifts on its own slow orbit around the centre.
			const o = f.spin * 0.35 + i * 2.094;
			el.style.opacity = (f.shells * 0.8).toFixed(4);
			el.style.transform =
				`translate3d(${(Math.cos(o) * spread).toFixed(2)}px, ${(Math.sin(o) * spread).toFixed(2)}px, 0) ` +
				`${s.base} ${s.axis}(${f.tilt.toFixed(2)}deg) rotateZ(${(a * DEG).toFixed(2)}deg)`;
		});
	};

	// Before the first paint, so a new orb never flashes at full size.
	useLayoutEffect(draw, []);

	useEffect(() => {
		const m = motion.current!;
		m.setReduced(reduced);
		m.setState(target);
		if (raf.current) return;
		// Compositor layers only while moving (orb.css), not for a resting orb.
		root.current?.classList.add("aio-live");
		let last = performance.now();
		const tick = (now: number) => {
			m.step((now - last) / 1000);
			last = now;
			draw();
			if (m.settled()) {
				raf.current = 0;
				root.current?.classList.remove("aio-live");
				if (m.state === "idle") onHiddenRef.current?.();
				return;
			}
			raf.current = requestAnimationFrame(tick);
		};
		raf.current = requestAnimationFrame(tick);
	}, [target, reduced]);

	useEffect(
		() => () => {
			cancelAnimationFrame(raf.current);
			raf.current = 0;
		},
		[]
	);

	return (
		<span ref={root} className="aio" style={{ "--aio-size": `${size}px` } as CSSProperties} aria-hidden>
			<span ref={glow} className="aio-glow" />
			<span className="aio-sphere">
				{SHELLS.map((_, i) => (
					<span key={i} ref={(el) => void (shells.current[i] = el)} className="aio-shell" />
				))}
				<span ref={tint} className="aio-tint" />
				<span ref={core} className="aio-core" />
			</span>
		</span>
	);
}
