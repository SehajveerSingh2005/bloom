interface VolumeLowIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function VolumeLowIcon({ size = 12, className, style }: VolumeLowIconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}>
      <path d="M11 5L6 9H2v6h4l5 4V5z" />
    </svg>
  );
}
