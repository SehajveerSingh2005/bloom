interface MusicNoteIconProps {
  size?: number;
  className?: string;
}

export function MusicNoteIcon({ size = 32, className }: MusicNoteIconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" className={className}>
      <path d="M18 3h-8v11.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h6v5.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V3z" />
    </svg>
  );
}
