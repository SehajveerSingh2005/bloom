import SpeakerWave1Svg from './sf/speaker.wave.1.fill.svg?react';

interface VolumeLowIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function VolumeLowIcon({ size = 12, className, style }: VolumeLowIconProps) {
  return <SpeakerWave1Svg width={size} height={size} className={className} style={style} />;
}
