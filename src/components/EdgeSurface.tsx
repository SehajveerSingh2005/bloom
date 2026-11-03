import { useId, useLayoutEffect, useRef } from "react";
import "./EdgeSurface.css";

type Edge = "top" | "bottom" | "left" | "right";

interface EdgeSurfaceProps {
	/** The screen edge the surface hangs from. */
	edge: Edge;
	/** Show the concave flares that bridge the pill to the edge. */
	flares?: boolean;
	/** Draw one continuous reflective contour around the surface. */
	reflective?: boolean;
}

/**
 * Background of a surface attached to a screen edge (the notch, the dock, the
 * volume and brightness overlays): the pill plus its two flares, drawn as one
 * layer. Render it as the first child of a positioned, isolated element that
 * sets `--surface-bg`; the pill takes that element's corner radii.
 */
export function EdgeSurface({ edge, flares = true, reflective = false }: EdgeSurfaceProps) {
	const surfaceRef = useRef<HTMLDivElement>(null);
	const rimRef = useRef<SVGPathElement>(null);
	const bevelRef = useRef<SVGPathElement>(null);
	const clipRef = useRef<SVGPathElement>(null);
	const gradientRef = useRef<SVGLinearGradientElement>(null);
	const rimGroupRef = useRef<SVGGElement>(null);
	const fadeStartRef = useRef<SVGStopElement>(null);
	const fadeEndRef = useRef<SVGStopElement>(null);
	const gradientId = useId();
	const clipId = `${gradientId}-clip`;
	const fadeId = `${gradientId}-fade`;
	const maskId = `${gradientId}-mask`;

	useLayoutEffect(() => {
		const surface = surfaceRef.current;
		const parent = surface?.parentElement;
		if (!reflective || !surface || !parent || (edge !== "top" && edge !== "bottom")) return;

		// Read the animated CSS radii and untransformed dimensions together.
		// Update only the SVG path: React need not render on each spring frame.
		let previousGeometry = "";
		const draw = () => {
			const style = getComputedStyle(parent);
			const surfaceStyle = getComputedStyle(surface);
			const height = parseFloat(surfaceStyle.height);
			const width = parseFloat(surfaceStyle.width);
			if (!(height > 0 && width > 26)) return;
			const bottom = edge === "bottom";
			const left = 13;
			const right = width - 13;
			const radius = (value: string) => Math.min(parseFloat(value) || 0, height / 2, (right - left) / 2);
			const rl = radius(bottom ? style.borderTopLeftRadius : style.borderBottomLeftRadius);
			const rr = radius(bottom ? style.borderTopRightRadius : style.borderBottomRightRadius);
			const backLeft = radius(style.borderBottomLeftRadius);
			const backRight = radius(style.borderBottomRightRadius);
			const geometry = [width, height, rl, rr, backLeft, backRight].join(",");
			if (geometry === previousGeometry) return;
			previousGeometry = geometry;
			// Author the notch contour, then mirror it for the dock. Its flare
			// uses exactly the same cubic as the existing surface fill.
			let path = flares ? `M 0 0 C 10 0 13 3 13 13` : `M ${left} 0`;
			path += ` L ${left} ${height - rl} A ${rl} ${rl} 0 0 0 ${left + rl} ${height}`;
			path += ` L ${right - rr} ${height} A ${rr} ${rr} 0 0 0 ${right} ${height - rr}`;
			path += flares ? ` L ${right} 13 C ${right} 3 ${width - 10} 0 ${width} 0` : ` L ${right} 0`;
			// A collapsed dock floats and has a fourth, rounded edge.
			if (bottom && !flares && (backLeft > 0 || backRight > 0)) {
				path = `M ${left} ${backLeft} A ${backLeft} ${backLeft} 0 0 1 ${left + backLeft} 0 L ${right - backRight} 0 A ${backRight} ${backRight} 0 0 1 ${right} ${backRight}`
					+ ` L ${right} ${height - rr} A ${rr} ${rr} 0 0 1 ${right - rr} ${height} L ${left + rl} ${height} A ${rl} ${rl} 0 0 1 ${left} ${height - rl} Z`;
			}
			// Paint real paths, rather than <use> shadow trees, so each geometry
			// change invalidates the visible strokes directly. The CSS fill
			// follows layout independently of these measured SVG decorations.
			for (const ref of [bevelRef, rimRef, clipRef]) {
				ref.current?.setAttribute("d", path);
			}
			rimGroupRef.current?.setAttribute("transform", bottom ? `translate(0 ${height}) scale(1 -1)` : "");
			gradientRef.current?.setAttribute("y2", String(height));
			// Fade only the concave tips, in a fixed 13px span at either end.
			fadeStartRef.current?.setAttribute("offset", String(13 / width));
			fadeEndRef.current?.setAttribute("offset", String(1 - 13 / width));
		};
		// Both observers run before paint. Deferring their work to another RAF
		// makes the SVG fill and rim trail the parent's spring by one frame.
		const resize = new ResizeObserver(draw);
		resize.observe(surface);
		const mutations = new MutationObserver(draw);
		mutations.observe(parent, { attributes: true, attributeFilter: ["style", "class"] });
		draw();
		return () => {
			resize.disconnect();
			mutations.disconnect();
		};
	}, [edge, flares, reflective]);

	return (
		<div ref={surfaceRef} className={`edge-surface edge-surface--${edge}${flares ? " show-flares" : ""}`} aria-hidden="true">
			<span className="edge-surface-flare edge-surface-flare--start" />
			<span className="edge-surface-flare edge-surface-flare--end" />
			{reflective && (
				<svg className="edge-surface-rim" width="100%" height="100%">
					<defs>
						<clipPath id={clipId}>
							<path ref={clipRef} />
						</clipPath>
						<linearGradient ref={gradientRef} id={gradientId} gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="1">
							<stop offset="0" stopColor="var(--surface-line)" />
							<stop offset="0.45" stopColor="var(--surface-side-light)" />
							<stop offset="1" stopColor="var(--surface-highlight)" />
						</linearGradient>
						<linearGradient id={fadeId}>
							<stop offset="0" stopColor="white" stopOpacity="0" />
							<stop ref={fadeStartRef} offset="0.1" stopColor="white" />
							<stop ref={fadeEndRef} offset="0.9" stopColor="white" />
							<stop offset="1" stopColor="white" stopOpacity="0" />
						</linearGradient>
						<mask id={maskId} maskUnits="userSpaceOnUse" x="0" y="0" width="100%" height="100%">
							<rect width="100%" height="100%" fill={`url(#${fadeId})`} />
						</mask>
					</defs>
					<g ref={rimGroupRef}>
						{/* A faint inner shoulder makes the rim read as reflected
						    light on a bevel rather than an isolated bright wire. */}
						<path ref={bevelRef} fill="none" stroke={`url(#${gradientId})`} strokeWidth="4" opacity="0.16"
							clipPath={`url(#${clipId})`} mask={flares ? `url(#${maskId})` : undefined} />
						{/* Clip a 2px centred stroke to leave a full 1px INSIDE
						    the fill, including at the viewport's horizontal edges. */}
						<path ref={rimRef} fill="none" stroke={`url(#${gradientId})`} strokeWidth="2"
							clipPath={`url(#${clipId})`} mask={flares ? `url(#${maskId})` : undefined} />
					</g>
				</svg>
			)}
		</div>
	);
}
