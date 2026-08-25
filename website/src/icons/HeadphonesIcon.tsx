interface HeadphonesIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function HeadphonesIcon({ size = 24, className, style }: HeadphonesIconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={className} style={style}>
      <path d="M3 18v-6a9 9 0 0 1 18 0v6" />
      <path d="M21 19a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3v5z" fill="currentColor" fillOpacity="0.15" />
      <path d="M3 19a2 2 0 0 0 2 2h1a2 2 0 0 0 2-2v-3a2 2 0 0 0-2-2H3v5z" fill="currentColor" fillOpacity="0.15" />
      <rect x="20" y="14" width="3" height="5" rx="1" />
      <rect x="1" y="14" width="3" height="5" rx="1" />
    </svg>
  );
}
