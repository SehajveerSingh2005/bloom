import { useEffect, useRef, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Check, ChevronLeft, ChevronRight } from "lucide-react";

export function calendarOffset(month: Date, mondayFirst = false): number {
	return (month.getDay() + (mondayFirst ? 6 : 0)) % 7;
}

export function calendarRows(month: Date, mondayFirst = false): number {
	return Math.ceil(
		(calendarOffset(month, mondayFirst) +
			new Date(month.getFullYear(), month.getMonth() + 1, 0).getDate()) /
			7
	);
}

const monthNames = Array.from({ length: 12 }, (_, month) =>
	new Date(2024, month, 1).toLocaleString("default", { month: "long" })
);

export function Calendar({
	month,
	mondayFirst = false,
	onMonthChange
}: {
	month: Date;
	mondayFirst?: boolean;
	onMonthChange: (month: Date) => void;
}) {
	const [today, setToday] = useState(() => new Date());
	const lastWheel = useRef(0);
	const [editing, setEditing] = useState(false);
	useEffect(() => {
		const interval = setInterval(() => setToday(new Date()), 60_000);
		return () => clearInterval(interval);
	}, []);
	const year = month.getFullYear();
	const monthIndex = month.getMonth();
	const changeMonth = (offset: number) => {
		const next = new Date(year, monthIndex + offset, 1);
		if (next.getFullYear() >= 100 && next.getFullYear() <= 9999) onMonthChange(next);
	};
	const goToday = () => {
		const now = new Date();
		setToday(now);
		onMonthChange(new Date(now.getFullYear(), now.getMonth(), 1));
	};
	const isCurrentMonth = year === today.getFullYear() && monthIndex === today.getMonth();
	const totalDays = new Date(year, monthIndex + 1, 0).getDate();

	return (
		<div
			className="calendar-container"
			tabIndex={0}
			aria-label="Calendar. Page Up and Page Down change month; hold Shift to change year. Home returns to today."
			onKeyDown={(event) => {
				if ((event.target as HTMLElement).matches("input, select, button")) return;
				if (event.key === "PageUp" || event.key === "PageDown") {
					event.preventDefault();
					changeMonth((event.key === "PageUp" ? -1 : 1) * (event.shiftKey ? 12 : 1));
				} else if (event.key === "Home") {
					event.preventDefault();
					goToday();
				}
			}}
			onWheel={(event) => {
				event.stopPropagation();
				if ((event.target as HTMLElement).closest("input, select")) return;
				const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
				if (Math.abs(delta) < 5 || Date.now() - lastWheel.current < 300) return;
				lastWheel.current = Date.now();
				changeMonth((delta > 0 ? 1 : -1) * (event.shiftKey ? 12 : 1));
			}}
		>
			<div
				className="calendar-header"
				onBlur={(event) => {
					if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setEditing(false);
				}}
				onKeyDown={(event) => {
					if (event.key === "Escape") setEditing(false);
				}}
			>
				{editing ? (
					<div className="calendar-month-picker">
						<select
							autoFocus
							aria-label="Month"
							value={monthIndex}
							onFocus={() => {
								void getCurrentWebviewWindow().setFocus().catch(console.error);
							}}
							onChange={(event) => onMonthChange(new Date(year, Number(event.target.value), 1))}
						>
							{monthNames.map((name, index) => (
								<option key={name} value={index}>
									{name}
								</option>
							))}
						</select>
						<input
							key={year}
							type="number"
							aria-label="Year"
							min={100}
							max={9999}
							defaultValue={year}
							onFocus={() => {
								void getCurrentWebviewWindow().setFocus().catch(console.error);
							}}
							onBlur={(event) => {
								const value = Number(event.currentTarget.value);
								if (Number.isInteger(value) && value >= 100 && value <= 9999)
									onMonthChange(new Date(value, monthIndex, 1));
								else event.currentTarget.value = String(year);
							}}
							onKeyDown={(event) => {
								if (event.key === "Enter") {
									event.currentTarget.blur();
									setEditing(false);
								}
							}}
						/>
						<button
							type="button"
							aria-label="Done choosing month"
							onClick={() => setEditing(false)}
						>
							<Check size={13} />
						</button>
					</div>
				) : (
					<>
						<button
							type="button"
							className="calendar-heading"
							onClick={() => setEditing(true)}
							title="Change month or year. Scroll to browse; Shift + scroll changes year."
						>
							{monthNames[monthIndex]} {year}
						</button>
						<div className="calendar-navigation" role="group" aria-label="Browse months">
							<button
								type="button"
								aria-label="Previous month"
								title="Previous month"
								onClick={() => changeMonth(-1)}
							>
								<ChevronLeft size={13} />
							</button>
							<button
								type="button"
								aria-label="Next month"
								title="Next month"
								onClick={() => changeMonth(1)}
							>
								<ChevronRight size={13} />
							</button>
						</div>
					</>
				)}
				<button
					type="button"
					className={`calendar-today${isCurrentMonth ? " is-current" : ""}`}
					disabled={isCurrentMonth}
					aria-hidden={isCurrentMonth}
					onClick={() => {
						goToday();
						setEditing(false);
					}}
				>
					Today
				</button>
			</div>
			<div
				className="calendar-grid"
				aria-label={month.toLocaleDateString("default", { month: "long", year: "numeric" })}
			>
				{(mondayFirst
					? ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
					: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
				).map((day) => (
					<div key={day} className="day-name">
						{day}
					</div>
				))}
				{Array.from({ length: calendarOffset(month, mondayFirst) }, (_, index) => (
					<div key={`empty-${index}`} aria-hidden="true" />
				))}
				{Array.from({ length: totalDays }, (_, index) => {
					const day = index + 1;
					const isToday = isCurrentMonth && day === today.getDate();
					return (
						<div
							key={day}
							className={`calendar-day${isToday ? " today" : ""}`}
							aria-current={isToday ? "date" : undefined}
						>
							{day}
						</div>
					);
				})}
			</div>
		</div>
	);
}
