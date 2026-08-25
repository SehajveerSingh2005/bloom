interface SkipBackIconProps {
  size?: number;
  className?: string;
}

export function SkipBackIcon({ size = 28, className }: SkipBackIconProps) {
  return (
    <svg width={size} height={size / 2} viewBox="0 0 66 32" fill="currentColor" className={className}>
      <g transform="scale(-1,1) translate(-66,0)">
        <path d="M 7.54 0.06 C 8.12 0.06 8.78 0.36 9.23 0.64 L 31.66 13.83 C 32.11 14.09 32.48 14.45 32.63 14.93 L 32.63 2.55 C 32.63 0.81 33.68 0.06 34.71 0.06 C 35.27 0.06 35.94 0.36 36.39 0.64 L 58.84 13.83 C 59.46 14.2 59.91 14.78 59.91 15.59 C 59.91 16.41 59.51 16.9 58.84 17.31 L 36.39 30.5 C 35.9 30.78 35.27 31.08 34.71 31.08 C 33.68 31.08 32.63 30.33 32.63 28.57 L 32.63 16.26 C 32.48 16.71 32.14 17.03 31.66 17.31 L 9.23 30.5 C 8.74 30.78 8.12 31.08 7.54 31.08 C 6.5 31.08 5.47 30.33 5.47 28.57 L 5.47 2.55 C 5.47 0.81 6.5 0.06 7.54 0.06 Z" />
      </g>
    </svg>
  );
}
