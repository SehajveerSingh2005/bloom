import SpeakerWave2Svg from './sf/speaker.wave.2.fill.svg?react';
import SpeakerSlashSvg from './sf/speaker.slash.fill.svg?react';

interface SpeakerIconProps {
  size?: number;
  className?: string;
  style?: React.CSSProperties;
  muted?: boolean;
}

export function SpeakerIcon({ size = 24, className, style, muted }: SpeakerIconProps) {
  return muted
    ? <SpeakerSlashSvg width={size} height={size} className={className} style={style} />
    : <SpeakerWave2Svg width={size} height={size} className={className} style={style} />;
}
