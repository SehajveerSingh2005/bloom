interface VolumeHighIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function VolumeHighIcon({ size = 12, className, style }: VolumeHighIconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}>
      <path d="M11 5L6 9H2v6h4l5 4V5z" fill="currentColor" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" strokeWidth="2.5" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" strokeWidth="2.5" />
    </svg>
  );
}
