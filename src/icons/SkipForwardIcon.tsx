import ForwardSvg from './sf/forward.fill.svg?react';

interface SkipForwardIconProps {
  size?: number;
  className?: string;
}

export function SkipForwardIcon({ size = 28, className }: SkipForwardIconProps) {
  return <ForwardSvg width={size} height={size / 2} className={className} />;
}
