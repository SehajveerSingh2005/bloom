import {
  useState,
  useEffect,
  useRef,
  useCallback,
  type ComponentType,
} from "react";
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
  type LucideProps,
} from "lucide-react";

// WMO weather interpretation codes
// https://open-meteo.com/en/docs#weathervariables
const WMO_CODES: Record<number, string> = {
  0: "Clear",
  1: "Mostly Clear",
  2: "Partly Cloudy",
  3: "Overcast",
  45: "Foggy",
  48: "Foggy",
  51: "Drizzle",
  53: "Drizzle",
  55: "Drizzle",
  56: "Freezing Drizzle",
  57: "Freezing Drizzle",
  61: "Rainy",
  63: "Rainy",
  65: "Rainy",
  66: "Freezing Rain",
  67: "Freezing Rain",
  71: "Snowy",
  73: "Snowy",
  75: "Snowy",
  77: "Snowy",
  80: "Rain Showers",
  81: "Rain Showers",
  82: "Rain Showers",
  85: "Snow Showers",
  86: "Snow Showers",
  95: "Stormy",
  96: "Stormy",
  99: "Stormy",
};

const DELHI_LAT = 28.6139;
const DELHI_LON = 77.2090;
const REFRESH_INTERVAL_MS = 30 * 60 * 1000; // 30 minutes

interface WeatherState {
  temperature: number | null;
  weatherCondition: string;
  weatherIcon: ComponentType<LucideProps>;
}

interface ResolvedLocation {
  lat: number;
  lon: number;
  city?: string;
}

function getWeatherIcon(
  condition: string,
  isDay = true,
): ComponentType<LucideProps> {
  switch (condition) {
    case "Clear":
    case "Mostly Clear":
      return isDay ? Sun : Moon;
    case "Partly Cloudy":
    case "Overcast":
      return Cloud;
    case "Foggy":
      return CloudFog;
    case "Drizzle":
    case "Freezing Drizzle":
      return CloudDrizzle;
    case "Rainy":
    case "Rain Showers":
    case "Freezing Rain":
      return CloudRain;
    case "Snowy":
    case "Snow Showers":
      return CloudSnow;
    case "Stormy":
      return CloudLightning;
    default:
      return Thermometer;
  }
}

async function fetchWeatherForCoords(
  latitude: number,
  longitude: number,
  unit: string,
): Promise<WeatherState> {
  const unitParam = unit === "fahrenheit" ? "&temperature_unit=fahrenheit" : "";
  const response = await fetch(
    `https://api.open-meteo.com/v1/forecast?latitude=${latitude}&longitude=${longitude}&current=temperature_2m,weather_code,is_day&timezone=auto${unitParam}`,
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
  const condition = WMO_CODES[code] || "Unknown";
  const isDay = data.current.is_day === 1;

  return {
    temperature: temp,
    weatherCondition: condition,
    weatherIcon: getWeatherIcon(condition, isDay),
  };
}

async function resolveLocation(): Promise<ResolvedLocation> {
  // 1. Check saved coordinates from settings or localStorage
  try {
    const settings = (await invoke("load_settings").catch(() => ({}))) as Record<
      string,
      any
    >;
    const savedLat =
      settings["bloom-weather-lat"] || localStorage.getItem("bloom-weather-lat");
    const savedLon =
      settings["bloom-weather-lon"] || localStorage.getItem("bloom-weather-lon");
    const savedCity =
      settings["bloom-weather-city"] || localStorage.getItem("bloom-weather-city");

    if (savedLat && savedLon) {
      return {
        lat: parseFloat(savedLat),
        lon: parseFloat(savedLon),
        city: savedCity || undefined,
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

  // 3. Fallback IP geolocation
  try {
    const res = await fetch(
      "https://ip-api.com/json/?fields=status,lat,lon,city,country",
    );
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

async function persistWeather(
  temp: number | null,
  condition: string,
): Promise<void> {
  if (temp !== null) {
    invoke("save_setting", {
      key: "bloom-weather-cached-temp",
      value: temp,
    }).catch(() => {});
  }
  if (condition) {
    invoke("save_setting", {
      key: "bloom-weather-cached-condition",
      value: condition,
    }).catch(() => {});
  }
}

export function useWeather(enabled: boolean) {
  const [temperature, setTemperature] = useState<number | null>(() => {
    const cached = localStorage.getItem("bloom-weather-cached-temp");
    return cached !== null ? Number(cached) : null;
  });
  const [weatherCondition, setWeatherCondition] = useState<string>(
    () => localStorage.getItem("bloom-weather-cached-condition") || "",
  );
  const [weatherIcon, setWeatherIcon] = useState<ComponentType<LucideProps>>(
    () => Thermometer,
  );
  const [cityName, setCityName] = useState<string>(
    () => localStorage.getItem("bloom-weather-city") || "",
  );
  const [tempUnit, setTempUnit] = useState<string>(
    () => localStorage.getItem("bloom-temp-unit") || "celsius",
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
        const location = coords
          ? { ...coords, city: undefined }
          : await resolveLocation();
        const result = await fetchWeatherForCoords(
          location.lat,
          location.lon,
          tempUnitRef.current,
        );

        setTemperature(result.temperature);
        setWeatherCondition(result.weatherCondition);
        setWeatherIcon(() => result.weatherIcon);
        persistWeather(result.temperature, result.weatherCondition);

        // Update city name if resolved from IP geolocation
        if (location.city) {
          setCityName(location.city);
          localStorage.setItem("bloom-weather-city", location.city);
        }
      } catch (e) {
        console.warn("Weather fetch failed:", e);
        if (!showStaleOnError && import.meta.env.DEV) {
          const mockTemp = tempUnitRef.current === "fahrenheit" ? 72 : 22;
          setTemperature(mockTemp);
          setWeatherCondition("Partly Cloudy");
          setWeatherIcon(() => Cloud);
        }
      }
    },
    [],
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
          setWeatherCondition(String(cachedCond));
          setWeatherIcon(() => getWeatherIcon(String(cachedCond)));
        }
        const savedUnit =
          settings["bloom-temp-unit"] ?? localStorage.getItem("bloom-temp-unit");
        if (savedUnit) {
          setTempUnit(String(savedUnit));
        }
        const savedCity =
          settings["bloom-weather-city"] ??
          localStorage.getItem("bloom-weather-city");
        if (savedCity) {
          setCityName(String(savedCity));
        }
      })
      .catch(() => {});
  }, []);

  // Listen for temp unit changes
  useEffect(() => {
    const unlisten = listen<{ key: string; value: any }>(
      "settings-changed",
      (event) => {
        const { key, value } = event.payload;
        if (key === "temp-unit") {
          setTempUnit(value ? "fahrenheit" : "celsius");
        }
      },
    );
    const unlistenExternal = listen<{ key: string; value: any }>(
      "settings-external-changed",
      (event) => {
        const { key, value } = event.payload;
        if (key === "bloom-temp-unit") {
          setTempUnit(String(value));
        }
      },
    );
    return () => {
      unlisten.then((fn) => fn());
      unlistenExternal.then((fn) => fn());
    };
  }, []);

  // Listen for instant refresh from Settings (direct Tauri event with coords)
  useEffect(() => {
    if (!enabled) return;

    const unlisten = listen<{ lat: number; lon: number } | true>(
      "weather-refresh",
      (event) => {
        const payload = event.payload;
        if (
          payload &&
          typeof payload === "object" &&
          "lat" in payload &&
          "lon" in payload
        ) {
          doFetch(false, { lat: (payload as any).lat, lon: (payload as any).lon });
        } else {
          doFetch(false);
        }
      },
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [enabled, doFetch]);

  // Listen for city name changes from Settings
  useEffect(() => {
    const unlisten = listen<{ key: string; value: any }>(
      "settings-changed",
      (event) => {
        const { key, value } = event.payload;
        if (key === "weather-city") {
          setCityName(String(value || ""));
        }
      },
    );
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
    weatherCondition,
    weatherIcon,
    cityName,
    tempUnit,
  };
}
