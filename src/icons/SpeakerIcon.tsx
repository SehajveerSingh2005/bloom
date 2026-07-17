interface SpeakerIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
  muted?: boolean;
}

export function SpeakerIcon({ size = 24, className, style, muted }: SpeakerIconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}>
      <path d="M11 5L6 9H2v6h4l5 4V5z" fill="currentColor" fillOpacity="0.15" />
      <path d="M11 5L6 9H2v6h4l5 4V5z" />
      {!muted && <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />}
      {!muted && <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />}
      {muted && <><line x1="22" y1="9" x2="17" y2="15" /><line x1="17" y1="9" x2="22" y2="15" /></>}
    </svg>
  );
}
