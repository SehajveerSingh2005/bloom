import { motion, useReducedMotion } from "framer-motion";
import { Check, CircleAlert, X } from "lucide-react";
import { AGENT_MESSAGES, AGENT_NAMES, type Agent, type AgentAlert as Alert } from "../agentAlerts";
import "./AgentAlert.css";

export function AgentLogo({ agent }: { agent: Agent }) {
	return (
		<span
			className={`agent-logo agent-${agent}`}
			style={{ maskImage: `url(/agents/${agent}.svg)` }}
			aria-hidden="true"
		/>
	);
}

export function AgentAlert({ alert, onDismiss }: { alert: Alert; onDismiss: () => void }) {
	const reducedMotion = useReducedMotion();
	return (
		<motion.div
			className={`agent-alert kind-${alert.kind}`}
			role="status"
			aria-live="polite"
			onClick={(event) => event.stopPropagation()}
			initial={{ opacity: 0, y: reducedMotion ? 0 : -6 }}
			animate={{ opacity: 1, y: 0 }}
			exit={{ opacity: 0 }}
		>
			<motion.div
				className="agent-logo-wrap"
				initial={false}
				animate={
					reducedMotion
						? {}
						: alert.kind === "attention"
							? { scale: [1, 1.07, 1] }
							: { rotate: [0, -12, 8, 0] }
				}
				transition={
					alert.kind === "attention" ? { duration: 2.4, repeat: Infinity } : { duration: 0.7 }
				}
			>
				<AgentLogo agent={alert.agent} />
			</motion.div>
			<div className="agent-alert-copy">
				<span className="agent-alert-source">
					{AGENT_NAMES[alert.agent]} <span>· {alert.project}</span>
				</span>
				<strong>{AGENT_MESSAGES[alert.kind]}</strong>
			</div>
			{alert.kind === "ready" ? (
				<Check className="agent-alert-state" size={16} />
			) : (
				<CircleAlert className="agent-alert-state" size={16} />
			)}
			<button
				type="button"
				className="agent-alert-dismiss"
				onClick={onDismiss}
				aria-label="Dismiss agent alert"
			>
				<X size={12} />
			</button>
		</motion.div>
	);
}
