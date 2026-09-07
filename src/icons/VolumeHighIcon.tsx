import SpeakerWave3Svg from './sf/speaker.wave.3.fill.svg?react';

interface VolumeHighIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
}

export function VolumeHighIcon({ size = 12, className, style }: VolumeHighIconProps) {
  return <SpeakerWave3Svg width={size} height={size} className={className} style={style} />;
}
