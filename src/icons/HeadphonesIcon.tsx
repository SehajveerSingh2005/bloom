import HeadphonesSvg from './sf/headphones.svg?react';

interface HeadphonesIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function HeadphonesIcon({ size = 24, className, style }: HeadphonesIconProps) {
  return <HeadphonesSvg width={size} height={size} className={className} style={style} />;
}
