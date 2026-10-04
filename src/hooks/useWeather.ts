import { useState, useEffect, useRef, useCallback, type ComponentType } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
	Sun,
	Moon,
	Cloud,
	CloudRain,
	CloudSnow,
	CloudLightning,
	CloudFog,
	CloudDrizzle,
	Thermometer,
	type LucideProps
} from "lucide-react";
import { t } from "../i18n";

// WMO weather interpretation codes
// https://open-meteo.com/en/docs#weathervariables
const WMO_CODES: Record<number, string> = {
	0: "weather.conditions.clear",
	1: "weather.conditions.mostlyClear",
	2: "weather.conditions.partlyCloudy",
	3: "weather.conditions.overcast",
	45: "weather.conditions.fog",
	48: "weather.conditions.fog",
	51: "weather.conditions.drizzle",
	53: "weather.conditions.drizzle",
	55: "weather.conditions.drizzle",
	56: "weather.conditions.freezingDrizzle",
	57: "weather.conditions.freezingDrizzle",
	61: "weather.conditions.rain",
	63: "weather.conditions.rain",
	65: "weather.conditions.rain",
	66: "weather.conditions.freezingRain",
	67: "weather.conditions.freezingRain",
	71: "weather.conditions.snow",
	73: "weather.conditions.snow",
	75: "weather.conditions.snow",
	77: "weather.conditions.snow",
	80: "weather.conditions.showers",
	81: "weather.conditions.showers",
	82: "weather.conditions.showers",
	85: "weather.conditions.snowShowers",
	86: "weather.conditions.snowShowers",
	95: "weather.conditions.thunderstorm",
	96: "weather.conditions.thunderstorm",
	99: "weather.conditions.thunderstorm"
};

// Condition values cached before i18n held English labels; map them back to
// keys so an upgraded install keeps the right label and icon until the next
// successful fetch.
const LEGACY_CONDITION_KEYS: Record<string, string> = {
	Clear: "weather.conditions.clear",
	"Mostly Clear": "weather.conditions.mostlyClear",
	"Partly Cloudy": "weather.conditions.partlyCloudy",
	Overcast: "weather.conditions.overcast",
	Foggy: "weather.conditions.fog",
	Drizzle: "weather.conditions.drizzle",
	"Freezing Drizzle": "weather.conditions.freezingDrizzle",
	Rainy: "weather.conditions.rain",
	"Freezing Rain": "weather.conditions.freezingRain",
	Snowy: "weather.conditions.snow",
	"Rain Showers": "weather.conditions.showers",
	"Snow Showers": "weather.conditions.snowShowers",
	Stormy: "weather.conditions.thunderstorm",
	Unknown: "weather.conditions.unknown"
};

function conditionKeyFromCache(value: string): string {
	if (!value || value.startsWith("weather.conditions.")) return value;
	return LEGACY_CONDITION_KEYS[value] ?? value;
}

const DELHI_LAT = 28.6139;
const DELHI_LON = 77.209;
const REFRESH_INTERVAL_MS = 30 * 60 * 1000; // 30 minutes

interface WeatherState {
	temperature: number | null;
	weatherConditionKey: string;
	weatherIcon: ComponentType<LucideProps>;
}

interface ResolvedLocation {
	lat: number;
	lon: number;
	city?: string;
}

function getWeatherIcon(conditionKey: string, isDay = true): ComponentType<LucideProps> {
	switch (conditionKey) {
		case "weather.conditions.clear":
		case "weather.conditions.mostlyClear":
			return isDay ? Sun : Moon;
		case "weather.conditions.partlyCloudy":
		case "weather.conditions.overcast":
			return Cloud;
		case "weather.conditions.fog":
			return CloudFog;
		case "weather.conditions.drizzle":
		case "weather.conditions.freezingDrizzle":
			return CloudDrizzle;
		case "weather.conditions.rain":
		case "weather.conditions.showers":
		case "weather.conditions.freezingRain":
			return CloudRain;
		case "weather.conditions.snow":
		case "weather.conditions.snowShowers":
			return CloudSnow;
		case "weather.conditions.thunderstorm":
			return CloudLightning;
		default:
			return Thermometer;
	}
}

async function fetchWeatherForCoords(
	latitude: number,
	longitude: number,
	unit: string
): Promise<WeatherState> {
	const unitParam = unit === "fahrenheit" ? "&temperature_unit=fahrenheit" : "";
	const response = await fetch(
		`https://api.open-meteo.com/v1/forecast?latitude=${latitude}&longitude=${longitude}&current=temperature_2m,weather_code,is_day&timezone=auto${unitParam}`
	);

	if (!response.ok) {
		throw new Error(`Weather API returned ${response.status}`);
	}

	const data = await response.json();

	if (!data?.current?.temperature_2m) {
		throw new Error("Invalid weather API response shape");
	}

	const temp = Math.round(data.current.temperature_2m);
	const code = data.current.weather_code;
	const conditionKey = WMO_CODES[code] || "weather.conditions.unknown";
	const isDay = data.current.is_day === 1;

	return {
		temperature: temp,
		weatherConditionKey: conditionKey,
		weatherIcon: getWeatherIcon(conditionKey, isDay)
	};
}

async function resolveLocation(): Promise<ResolvedLocation> {
	// 1. Check saved coordinates from settings or localStorage
	try {
		const settings = (await invoke("load_settings").catch(() => ({}))) as Record<string, any>;
		const savedLat = settings["bloom-weather-lat"] || localStorage.getItem("bloom-weather-lat");
		const savedLon = settings["bloom-weather-lon"] || localStorage.getItem("bloom-weather-lon");
		const savedCity = settings["bloom-weather-city"] || localStorage.getItem("bloom-weather-city");

		if (savedLat && savedLon) {
			return {
				lat: parseFloat(savedLat),
				lon: parseFloat(savedLon),
				city: savedCity || undefined
			};
		}
	} catch {
		// continue to IP geolocation
	}

	// 2. Try IP-based geolocation
	try {
		const res = await fetch("https://ipapi.co/json/");
		if (res.ok) {
			const data = await res.json();
			const lat = data.latitude || data.lat;
			const lon = data.longitude || data.lon;
			if (lat && lon) {
				return { lat, lon, city: data.city || undefined };
			}
		}
	} catch {
		// try fallback
	}

	// 3. Fallback IP geolocation. ip-api.com's free tier is HTTP-only: the
	// https:// endpoint answers 403 unless the request carries a paid key.
	try {
		const res = await fetch("http://ip-api.com/json/?fields=status,lat,lon,city,country");
		if (res.ok) {
			const data = await res.json();
			if (data.lat && data.lon) {
				return { lat: data.lat, lon: data.lon, city: data.city || undefined };
			}
		}
	} catch {
		// fall through to default
	}

	// 4. Default to Delhi
	return { lat: DELHI_LAT, lon: DELHI_LON, city: "Delhi" };
}

async function persistWeather(temp: number | null, condition: string): Promise<void> {
	if (temp !== null) {
		invoke("save_setting", {
			key: "bloom-weather-cached-temp",
			value: temp
		}).catch(() => {});
	}
	if (condition) {
		invoke("save_setting", {
			key: "bloom-weather-cached-condition",
			value: condition
		}).catch(() => {});
	}
}

export function useWeather(enabled: boolean) {
	const [temperature, setTemperature] = useState<number | null>(() => {
		const cached = localStorage.getItem("bloom-weather-cached-temp");
		return cached !== null ? Number(cached) : null;
	});
	const [weatherConditionKey, setWeatherConditionKey] = useState<string>(() =>
		conditionKeyFromCache(localStorage.getItem("bloom-weather-cached-condition") || "")
	);
	const [weatherIcon, setWeatherIcon] = useState<ComponentType<LucideProps>>(() => Thermometer);
	const [cityName, setCityName] = useState<string>(
		() => localStorage.getItem("bloom-weather-city") || ""
	);
	const [tempUnit, setTempUnit] = useState<string>(
		() => localStorage.getItem("bloom-temp-unit") || "celsius"
	);

	const tempUnitRef = useRef(tempUnit);
	tempUnitRef.current = tempUnit;

	const enabledRef = useRef(enabled);
	enabledRef.current = enabled;

	// Core fetch + refresh logic
	const doFetch = useCallback(
		async (showStaleOnError = true, coords?: { lat: number; lon: number }) => {
			if (!enabledRef.current) return;

			try {
				const location = coords ? { ...coords, city: undefined } : await resolveLocation();
				const result = await fetchWeatherForCoords(location.lat, location.lon, tempUnitRef.current);

				setTemperature(result.temperature);
				setWeatherConditionKey(result.weatherConditionKey);
				setWeatherIcon(() => result.weatherIcon);
				persistWeather(result.temperature, result.weatherConditionKey);

				// Update city name if resolved from IP geolocation
				if (location.city) {
					setCityName(location.city);
					localStorage.setItem("bloom-weather-city", location.city);
					invoke("save_setting", { key: "bloom-weather-city", value: location.city }).catch(
						() => {}
					);
				}
			} catch (e) {
				console.warn("Weather fetch failed:", e);
				if (!showStaleOnError && import.meta.env.DEV) {
					const mockTemp = tempUnitRef.current === "fahrenheit" ? 72 : 22;
					setTemperature(mockTemp);
					setWeatherConditionKey("weather.conditions.partlyCloudy");
					setWeatherIcon(() => Cloud);
				}
			}
		},
		[]
	);

	// Load cached values from Tauri settings on mount
	useEffect(() => {
		invoke("load_settings")
			.then((settings: any) => {
				const cachedTemp =
					settings["bloom-weather-cached-temp"] ??
					localStorage.getItem("bloom-weather-cached-temp");
				if (cachedTemp !== undefined && cachedTemp !== null) {
					setTemperature(Number(cachedTemp));
				}
				const cachedCond =
					settings["bloom-weather-cached-condition"] ??
					localStorage.getItem("bloom-weather-cached-condition");
				if (cachedCond) {
					const conditionKey = conditionKeyFromCache(String(cachedCond));
					setWeatherConditionKey(conditionKey);
					setWeatherIcon(() => getWeatherIcon(conditionKey));
				}
				const savedUnit = settings["bloom-temp-unit"] ?? localStorage.getItem("bloom-temp-unit");
				if (savedUnit) {
					setTempUnit(String(savedUnit));
				}
				const savedCity =
					settings["bloom-weather-city"] ?? localStorage.getItem("bloom-weather-city");
				if (savedCity) {
					setCityName(String(savedCity));
				}
			})
			.catch(() => {});
	}, []);

	// Listen for temp unit changes
	useEffect(() => {
		const unlisten = listen<{ key: string; value: any }>("settings-changed", (event) => {
			const { key, value } = event.payload;
			if (key === "temp-unit") {
				setTempUnit(value ? "fahrenheit" : "celsius");
			}
		});
		const unlistenExternal = listen<{ key: string; value: any }>(
			"settings-external-changed",
			(event) => {
				const { key, value } = event.payload;
				if (key === "bloom-temp-unit") {
					setTempUnit(String(value));
				}
			}
		);
		return () => {
			unlisten.then((fn) => fn());
			unlistenExternal.then((fn) => fn());
		};
	}, []);

	// Listen for instant refresh from Settings (direct Tauri event with coords)
	useEffect(() => {
		if (!enabled) return;

		const unlisten = listen<{ lat: number; lon: number } | true>("weather-refresh", (event) => {
			const payload = event.payload;
			if (payload && typeof payload === "object" && "lat" in payload && "lon" in payload) {
				doFetch(false, { lat: (payload as any).lat, lon: (payload as any).lon });
			} else {
				doFetch(false);
			}
		});
		return () => {
			unlisten.then((fn) => fn());
		};
	}, [enabled, doFetch]);

	// Listen for city name changes from Settings
	useEffect(() => {
		const unlisten = listen<{ key: string; value: any }>("settings-changed", (event) => {
			const { key, value } = event.payload;
			if (key === "weather-city") {
				setCityName(String(value || ""));
			}
		});
		return () => {
			unlisten.then((fn) => fn());
		};
	}, []);

	// Initial fetch + 30-min interval
	useEffect(() => {
		if (!enabled) return;

		doFetch(false);

		const interval = setInterval(() => doFetch(false), REFRESH_INTERVAL_MS);
		return () => clearInterval(interval);
	}, [enabled, doFetch]);

	// Re-fetch when temperature unit changes (without resetting interval)
	useEffect(() => {
		if (!enabled) return;
		doFetch(true);
	}, [tempUnit, enabled, doFetch]);

	return {
		temperature,
		weatherCondition: weatherConditionKey ? t(weatherConditionKey) : "",
		weatherIcon,
		cityName,
		tempUnit
	};
}
