import type { ComponentType, SVGProps } from "react";

export interface WidgetConfig {
  left: string[];
  right: string[];
}

export type SettingsTab = "general" | "appearance" | "notch" | "dock" | "overlays" | "about";

export interface SettingRowProps {
  icon: ComponentType<SVGProps<SVGSVGElement> & { size?: number; strokeWidth?: number }>;
  label: string;
  desc?: string;
  action?: boolean;
  danger?: boolean;
  divider?: boolean;
  onClick?: () => void;
  children?: React.ReactNode;
}
