import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { motion, useIsPresent, useReducedMotion } from "framer-motion";
import {
	AGENT_ACTIVITIES,
	agentActivityWidth,
	AGENT_MESSAGES,
	AGENT_NAMES,
	formatAgentElapsed,
	type Agent,
	type AgentAlert as Alert
} from "../agentAlerts";
import "./AgentAlert.css";

export function AgentLogo({ agent }: { agent: Agent }) {
	return (
		<span className={`agent-logo agent-${agent}`} aria-hidden="true">
			<img className="agent-logo-dark" src={`/agents/${agent}.svg`} alt="" draggable={false} />
			{agent !== "claude" && (
				<img
					className="agent-logo-light"
					src={`/agents/${agent}-light.svg`}
					alt=""
					draggable={false}
				/>
			)}
		</span>
	);
}

function Elapsed({ started, ended }: { started: number; ended?: number }) {
	const [now, setNow] = useState(Date.now);
	useEffect(() => {
		if (ended !== undefined) return;
		setNow(Date.now());
		const timer = setInterval(() => setNow(Date.now()), 1000);
		return () => clearInterval(timer);
	}, [started, ended]);
	return (
		<span className="agent-elapsed" aria-hidden="true">
			{formatAgentElapsed(started, ended ?? now)}
		</span>
	);
}

export function AgentAlert({
	alert,
	onWidthChange
}: {
	alert: Alert & { started?: number; received?: number };
	onWidthChange?: (width: number) => void;
}) {
	const isPresent = useIsPresent();
	const containerRef = useRef<HTMLDivElement>(null);
	useLayoutEffect(() => {
		const container = containerRef.current;
		if (!container || !onWidthChange) return;
		const status = container.querySelector<HTMLElement>(".agent-activity-status");
		const elapsed = container.querySelector<HTMLElement>(".agent-elapsed");
		if (!status || !elapsed) return;
		const measure = () => onWidthChange(agentActivityWidth(status.scrollWidth, elapsed.offsetWidth));
		const observer = new ResizeObserver(measure);
		observer.observe(status);
		observer.observe(elapsed);
		measure();
		return () => observer.disconnect();
	}, [onWidthChange, alert.kind, alert.activity]);
	const reducedMotion = useReducedMotion();
	const [fallbackStart] = useState(Date.now);
	const working = alert.kind === "working";
	const label = working
		? AGENT_ACTIVITIES[alert.activity ?? "thinking"]
		: AGENT_MESSAGES[alert.kind];
	const description = `${AGENT_NAMES[alert.agent]} · ${alert.project}: ${label}`;
	return (
		<motion.div
			ref={containerRef}
			className={`agent-alert kind-${alert.kind}`}
			data-activity={working ? (alert.activity ?? "thinking") : undefined}
			style={{ pointerEvents: isPresent ? "auto" : "none" }}
			role="status"
			aria-live="polite"
			aria-label={description}
			initial={{ opacity: 0 }}
			animate={{ opacity: 1 }}
			exit={{ opacity: 0 }}
			transition={{ duration: reducedMotion ? 0 : 0.15 }}
		>
			<div className="agent-activity-label">
				<div className={`agent-logo-wrap${working ? " is-working" : ""}`}>
					<AgentLogo agent={alert.agent} />
				</div>
				<span className="agent-activity-status">{label}</span>
			</div>
			<Elapsed
				started={alert.started ?? fallbackStart}
				ended={working ? undefined : (alert.received ?? fallbackStart)}
			/>
		</motion.div>
	);
}
