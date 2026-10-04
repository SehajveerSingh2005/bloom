import type { CSSProperties } from "react";
import type { AiPhase } from "./aiState";
import "./orb.css";

/**
 * Janice's orb, the same everywhere: small in the panel's status line, large
 * on screen for a "Hey <name>" request. Pure CSS (orb.css): it only moves
 * while the phase is active.
 */
export function AiOrb({ phase, size }: { phase: AiPhase; size: number }) {
	return (
		<span className="aio" data-phase={phase} style={{ "--aio-size": `${size}px` } as CSSProperties} aria-hidden>
			<span className="aio-glow" />
			<span className="aio-sphere">
				<span className="aio-shell" />
				<span className="aio-shell" />
				<span className="aio-shell" />
				<span className="aio-core" />
			</span>
		</span>
	);
}
