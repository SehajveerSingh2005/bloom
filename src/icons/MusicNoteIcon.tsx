import MusicNoteSvg from './sf/music.note.svg?react';

interface MusicNoteIconProps {
  size?: number;
  className?: string;
}

export function MusicNoteIcon({ size = 32, className }: MusicNoteIconProps) {
  return <MusicNoteSvg width={size} height={size} className={className} />;
}
