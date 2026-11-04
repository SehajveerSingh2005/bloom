import { CalendarDays } from "lucide-react";
import { AgentIntegrations } from "./AgentIntegrations";
import "./IntegrationsTab.css";

interface IntegrationsTabProps {
	agentAlertsEnabled: boolean;
	onToggleAgentAlerts: () => void;
}

export function IntegrationsTab({ agentAlertsEnabled, onToggleAgentAlerts }: IntegrationsTabProps) {
	return (
		<>
			<section className="integration-category" aria-labelledby="agent-integrations-title">
				<div className="integration-category-heading">
					<h2 id="agent-integrations-title">AI agents</h2>
					<p>Follow your agents’ progress and get notified when they need you.</p>
				</div>
				<AgentIntegrations enabled={agentAlertsEnabled} onToggle={onToggleAgentAlerts} />
			</section>
			<section className="integration-category" aria-labelledby="calendar-integrations-title">
				<div className="integration-category-heading">
					<h2 id="calendar-integrations-title">Calendars</h2>
					<p>Bring your schedule into Bloom.</p>
				</div>
				<div className="setting-group">
					<div className="integration-planned">
						<CalendarDays size={22} strokeWidth={1.5} aria-hidden="true" />
						<div>
							<strong>Google Calendar</strong>
							<p>Calendar connections are planned for a future update.</p>
						</div>
						<span className="integration-planned-badge">Planned</span>
					</div>
				</div>
			</section>
		</>
	);
}
