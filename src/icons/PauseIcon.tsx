import PauseSvg from './sf/pause.fill.svg?react';

interface PauseIconProps {
  size?: number;
  className?: string;
}

export function PauseIcon({ size = 14, className }: PauseIconProps) {
  return <PauseSvg width={size} height={size} className={className} />;
}
