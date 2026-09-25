import SliderHorizontal3Svg from "./sf/slider.horizontal.3.svg?react";

interface MixerIconProps {
	size?: number;
	className?: string;
	style?: React.CSSProperties;
}

export function MixerIcon({ size = 16, className, style }: MixerIconProps) {
	return <SliderHorizontal3Svg width={size} height={size} className={className} style={style} />;
}
