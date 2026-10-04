import React from "react";
import ReactDOM from "react-dom/client";
import Dock from "./Dock";
import { initI18n } from "./i18n";
import "./Dock.css";

document.addEventListener("contextmenu", (e) => e.preventDefault());

initI18n();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
	<React.StrictMode>
		<Dock />
	</React.StrictMode>
);
