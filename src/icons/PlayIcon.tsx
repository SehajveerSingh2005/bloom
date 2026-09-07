import PlaySvg from './sf/play.fill.svg?react';

interface PlayIconProps {
  size?: number;
  className?: string;
}

export function PlayIcon({ size = 14, className }: PlayIconProps) {
  return <PlaySvg width={size} height={size} className={className} style={{ marginLeft: '1px' }} />;
}
