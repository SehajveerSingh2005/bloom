import BackwardSvg from './sf/backward.fill.svg?react';

interface SkipBackIconProps {
  size?: number;
  className?: string;
}

export function SkipBackIcon({ size = 28, className }: SkipBackIconProps) {
  return <BackwardSvg width={size} height={size / 2} className={className} />;
}
