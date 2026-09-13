import { Monitor, Eye, EyeOff, Circle } from "lucide-react";
import { SettingRow } from "./SettingRow";

interface DockTabProps {
  dockEnabled: boolean;
  toggleDock: () => void;
  dockMode: string;
  setDockModeValue: (mode: string) => void;
  dockPreviewEnabled: boolean;
  toggleDockPreview: () => void;
  dockIconOnly: boolean;
  toggleDockIconOnly: () => void;
}

export function DockTab({
  dockEnabled,
  toggleDock,
  dockMode,
  setDockModeValue,
  dockPreviewEnabled,
  toggleDockPreview,
  dockIconOnly,
  toggleDockIconOnly,
}: DockTabProps) {
  return (
    <>
      <div className="setting-group-label">Dock</div>
      <div className="setting-group">
        <SettingRow icon={Monitor} label="Bloom Dock" desc="Replace Windows taskbar">
          <label className="toggle-switch">
            <input type="checkbox" checked={dockEnabled} onChange={toggleDock} />
            <span className="slider"></span>
          </label>
        </SettingRow>

        {dockEnabled && (
          <>
            <SettingRow icon={dockMode === "fixed" ? EyeOff : Eye} label="Behavior" desc="Choose how the dock appears">
              <select
                className="settings-select"
                value={dockMode}
                onChange={(e) => setDockModeValue(e.target.value)}
              >
                <option value="fixed">Fixed</option>
                <option value="smart">Smart</option>
                <option value="peek">Peek</option>
              </select>
            </SettingRow>

            <SettingRow icon={Eye} label="Show App Previews" desc="Show window thumbnails on hover">
              <label className="toggle-switch">
                <input type="checkbox" checked={dockPreviewEnabled} onChange={toggleDockPreview} />
                <span className="slider"></span>
              </label>
            </SettingRow>

            <SettingRow icon={Circle} label="Icon Only" desc="Remove icon background and padding" divider={false}>
              <label className="toggle-switch">
                <input type="checkbox" checked={dockIconOnly} onChange={toggleDockIconOnly} />
                <span className="slider"></span>
              </label>
            </SettingRow>
          </>
        )}
      </div>
    </>
  );
}
