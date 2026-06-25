import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Convert a hex color string to rgba with a specified alpha
function hexToRgba(hex: string, alpha: number): string {
  hex = hex.replace('#', '');
  if (hex.length === 3) {
    hex = hex.split('').map(char => char + char).join('');
  }
  const r = parseInt(hex.substring(0, 2), 16) || 0;
  const g = parseInt(hex.substring(2, 4), 16) || 0;
  const b = parseInt(hex.substring(4, 6), 16) || 0;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

// Calculate relative luminance to determine text color (W3C standard formula)
function isColorLight(hex: string): boolean {
  hex = hex.replace('#', '');
  if (hex.length === 3) {
    hex = hex.split('').map(char => char + char).join('');
  }
  const r = parseInt(hex.substring(0, 2), 16) || 0;
  const g = parseInt(hex.substring(2, 4), 16) || 0;
  const b = parseInt(hex.substring(4, 6), 16) || 0;
  const luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luminance > 0.55;
}

export interface HSL {
  h: number;
  s: number;
  l: number;
}

// Convert Hex to HSL
export function hexToHsl(hex: string): HSL {
  hex = hex.replace('#', '');
  if (hex.length === 3) {
    hex = hex.split('').map(char => char + char).join('');
  }
  const r = (parseInt(hex.substring(0, 2), 16) || 0) / 255;
  const g = (parseInt(hex.substring(2, 4), 16) || 0) / 255;
  const b = (parseInt(hex.substring(4, 6), 16) || 0) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let h = 0;
  let s = 0;
  const l = (max + min) / 2;

  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r:
        h = (g - b) / d + (g < b ? 6 : 0);
        break;
      case g:
        h = (b - r) / d + 2;
        break;
      case b:
        h = (r - g) / d + 4;
        break;
    }
    h /= 6;
  }

  return { h: Math.round(h * 360), s: Math.round(s * 100), l: Math.round(l * 100) };
}

// Convert HSL back to Hex color string
function hslToHex(h: number, s: number, l: number): string {
  s /= 100;
  l /= 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs((h / 60) % 2 - 1));
  const m = l - c / 2;
  let r = 0, g = 0, b = 0;

  if (0 <= h && h < 60) {
    r = c; g = x; b = 0;
  } else if (60 <= h && h < 120) {
    r = x; g = c; b = 0;
  } else if (120 <= h && h < 180) {
    r = 0; g = c; b = x;
  } else if (180 <= h && h < 240) {
    r = 0; g = x; b = c;
  } else if (240 <= h && h < 300) {
    r = x; g = 0; b = c;
  } else if (300 <= h && h < 360) {
    r = c; g = 0; b = x;
  }

  const toHex = (val: number) => {
    const hexVal = Math.round((val + m) * 255).toString(16);
    return hexVal.length === 1 ? '0' + hexVal : hexVal;
  };

  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

export async function applyTheme(
  mode: string,
  customColor: string,
  opacity?: number,
  saturation?: number,
  brightness?: number
) {
  const root = document.documentElement;

  if (opacity === undefined || opacity === null) {
    const cachedOpacity = localStorage.getItem('bloom-theme-opacity');
    opacity = cachedOpacity !== null ? parseFloat(cachedOpacity) : 0.80;
  }

  if (saturation === undefined || saturation === null) {
    const cachedSaturation = localStorage.getItem('bloom-theme-saturation');
    saturation = cachedSaturation !== null ? parseFloat(cachedSaturation) : 0.50;
  }

  if (brightness === undefined || brightness === null) {
    const cachedBrightness = localStorage.getItem('bloom-theme-brightness');
    brightness = cachedBrightness !== null ? parseFloat(cachedBrightness) : 0.15;
  }

  const op = opacity;
  const opExpanded = Math.min(1.0, opacity + 0.12);

  if (mode === 'light') {
    root.style.setProperty('--bloom-bg', `rgba(255, 255, 255, ${op})`);
    root.style.setProperty('--bloom-bg-expanded', `rgba(245, 245, 247, ${opExpanded})`);
    root.style.setProperty('--bloom-text', '#1c1c1e');
    root.style.setProperty('--bloom-text-muted', 'rgba(28, 28, 30, 0.65)');
    root.style.setProperty('--bloom-border', 'rgba(0, 0, 0, 0.12)');
    root.style.setProperty('--bloom-group-bg', 'rgba(0, 0, 0, 0.04)');
    root.style.setProperty('--bloom-accent', '#007aff');
    root.style.setProperty('--bloom-scrollbar-thumb', 'rgba(0, 0, 0, 0.15)');
    root.classList.add('light-mode');
    root.classList.remove('dark-mode');
    root.classList.add('theme-light');
    root.classList.remove('theme-dark', 'theme-custom', 'theme-adaptive');
  } else if (mode === 'custom') {
    const hsl = hexToHsl(customColor);
    hsl.s = saturation * 100;
    hsl.l = brightness * 100;
    const finalBgColor = hslToHex(hsl.h, hsl.s, hsl.l);
    const isLight = brightness > 0.55;

    root.style.setProperty('--bloom-bg', hexToRgba(finalBgColor, op));
    root.style.setProperty('--bloom-bg-expanded', hexToRgba(finalBgColor, opExpanded));
    root.style.setProperty('--bloom-text', isLight ? '#1c1c1e' : '#ffffff');
    root.style.setProperty('--bloom-text-muted', isLight ? 'rgba(28, 28, 30, 0.65)' : 'rgba(255, 255, 255, 0.65)');
    root.style.setProperty('--bloom-border', isLight ? 'rgba(0, 0, 0, 0.12)' : 'rgba(255, 255, 255, 0.12)');
    root.style.setProperty('--bloom-group-bg', isLight ? 'rgba(0, 0, 0, 0.04)' : 'rgba(255, 255, 255, 0.04)');
    root.style.setProperty('--bloom-accent', customColor);
    root.style.setProperty('--bloom-scrollbar-thumb', isLight ? 'rgba(0, 0, 0, 0.15)' : 'rgba(255, 255, 255, 0.15)');
    root.classList.add('theme-custom');
    root.classList.remove('theme-light', 'theme-dark', 'theme-adaptive');
    if (isLight) {
      root.classList.add('light-mode');
      root.classList.remove('dark-mode');
    } else {
      root.classList.add('dark-mode');
      root.classList.remove('light-mode');
    }
  } else if (mode === 'adaptive') {
    try {
      const accentHex = await invoke<string>('get_system_accent_color');
      const hsl = hexToHsl(accentHex);
      hsl.s = saturation * 100;
      hsl.l = brightness * 100;
      const finalBgColor = hslToHex(hsl.h, hsl.s, hsl.l);
      const isLight = brightness > 0.55;

      root.style.setProperty('--bloom-bg', hexToRgba(finalBgColor, op));
      root.style.setProperty('--bloom-bg-expanded', hexToRgba(finalBgColor, opExpanded));
      root.style.setProperty('--bloom-text', isLight ? '#1c1c1e' : '#ffffff');
      root.style.setProperty('--bloom-text-muted', isLight ? 'rgba(28, 28, 30, 0.65)' : 'rgba(255, 255, 255, 0.65)');
      root.style.setProperty('--bloom-border', isLight ? 'rgba(0, 0, 0, 0.12)' : 'rgba(255, 255, 255, 0.12)');
      root.style.setProperty('--bloom-group-bg', isLight ? 'rgba(0, 0, 0, 0.04)' : 'rgba(255, 255, 255, 0.04)');
      root.style.setProperty('--bloom-accent', accentHex);
      root.style.setProperty('--bloom-scrollbar-thumb', isLight ? 'rgba(0, 0, 0, 0.15)' : 'rgba(255, 255, 255, 0.15)');
      root.classList.add('theme-adaptive');
      root.classList.remove('theme-light', 'theme-dark', 'theme-custom');
      if (isLight) {
        root.classList.add('light-mode');
        root.classList.remove('dark-mode');
      } else {
        root.classList.add('dark-mode');
        root.classList.remove('light-mode');
      }
    } catch (e) {
      console.error("Failed to apply adaptive theme:", e);
      applyTheme('dark', customColor, op, saturation, brightness);
    }
  } else {
    // default/dark
    root.style.setProperty('--bloom-bg', `rgba(0, 0, 0, ${op})`);
    root.style.setProperty('--bloom-bg-expanded', `rgba(0, 0, 0, ${opExpanded})`);
    root.style.setProperty('--bloom-text', '#ffffff');
    root.style.setProperty('--bloom-text-muted', 'rgba(255, 255, 255, 0.6)');
    root.style.setProperty('--bloom-border', 'rgba(255, 255, 255, 0.1)');
    root.style.setProperty('--bloom-group-bg', 'rgba(255, 255, 255, 0.04)');
    root.style.setProperty('--bloom-accent', '#007aff');
    root.style.setProperty('--bloom-scrollbar-thumb', 'rgba(255, 255, 255, 0.1)');
    root.classList.add('dark-mode');
    root.classList.remove('light-mode');
    root.classList.add('theme-dark');
    root.classList.remove('theme-light', 'theme-custom', 'theme-adaptive');
  }
}

export function initTheme() {
  const syncMode = localStorage.getItem('bloom-theme-mode') || 'dark';
  const syncColor = localStorage.getItem('bloom-theme-color') || '#007aff';
  const syncOpacityVal = localStorage.getItem('bloom-theme-opacity');
  const syncOpacity = syncOpacityVal !== null ? parseFloat(syncOpacityVal) : 0.80;
  const syncSaturationVal = localStorage.getItem('bloom-theme-saturation');
  const syncSaturation = syncSaturationVal !== null ? parseFloat(syncSaturationVal) : 0.50;
  const syncBrightnessVal = localStorage.getItem('bloom-theme-brightness');
  const syncBrightness = syncBrightnessVal !== null ? parseFloat(syncBrightnessVal) : 0.15;

  // Fast synchronous draw using cached localStorage
  applyTheme(syncMode, syncColor, syncOpacity, syncSaturation, syncBrightness);

  // Sync settings.json in background to avoid flickers
  invoke<Record<string, any>>('load_settings').then((settings) => {
    const mode = settings['bloom-theme-mode'] ? String(settings['bloom-theme-mode']) : syncMode;
    const color = settings['bloom-theme-color'] ? String(settings['bloom-theme-color']) : syncColor;
    const opacityVal = settings['bloom-theme-opacity'] ? parseFloat(String(settings['bloom-theme-opacity'])) : syncOpacity;
    const saturationVal = settings['bloom-theme-saturation'] ? parseFloat(String(settings['bloom-theme-saturation'])) : syncSaturation;
    const brightnessVal = settings['bloom-theme-brightness'] ? parseFloat(String(settings['bloom-theme-brightness'])) : syncBrightness;

    if (
      mode !== syncMode ||
      color !== syncColor ||
      opacityVal !== syncOpacity ||
      saturationVal !== syncSaturation ||
      brightnessVal !== syncBrightness
    ) {
      localStorage.setItem('bloom-theme-mode', mode);
      localStorage.setItem('bloom-theme-color', color);
      localStorage.setItem('bloom-theme-opacity', String(opacityVal));
      localStorage.setItem('bloom-theme-saturation', String(saturationVal));
      localStorage.setItem('bloom-theme-brightness', String(brightnessVal));
      applyTheme(mode, color, opacityVal, saturationVal, brightnessVal);
    }
  }).catch(console.error);

  // Listen to setting changes broadcasted from settings window
  const settingsPromise = listen<{ key: string; value: any }>('settings-changed', (event) => {
    const { key, value } = event.payload;
    if (key === 'theme-mode') {
      localStorage.setItem('bloom-theme-mode', value);
      const color = localStorage.getItem('bloom-theme-color') || '#007aff';
      const opacity = parseFloat(localStorage.getItem('bloom-theme-opacity') || '0.80');
      const saturation = parseFloat(localStorage.getItem('bloom-theme-saturation') || '0.50');
      const brightness = parseFloat(localStorage.getItem('bloom-theme-brightness') || '0.15');
      applyTheme(value, color, opacity, saturation, brightness);
    }
    if (key === 'theme-color') {
      localStorage.setItem('bloom-theme-color', value);
      const mode = localStorage.getItem('bloom-theme-mode') || 'dark';
      const opacity = parseFloat(localStorage.getItem('bloom-theme-opacity') || '0.80');
      const saturation = parseFloat(localStorage.getItem('bloom-theme-saturation') || '0.50');
      const brightness = parseFloat(localStorage.getItem('bloom-theme-brightness') || '0.15');
      applyTheme(mode, value, opacity, saturation, brightness);
    }
    if (key === 'theme-opacity') {
      localStorage.setItem('bloom-theme-opacity', String(value));
      const mode = localStorage.getItem('bloom-theme-mode') || 'dark';
      const color = localStorage.getItem('bloom-theme-color') || '#007aff';
      const saturation = parseFloat(localStorage.getItem('bloom-theme-saturation') || '0.50');
      const brightness = parseFloat(localStorage.getItem('bloom-theme-brightness') || '0.15');
      applyTheme(mode, color, Number(value), saturation, brightness);
    }
    if (key === 'theme-saturation') {
      localStorage.setItem('bloom-theme-saturation', String(value));
      const mode = localStorage.getItem('bloom-theme-mode') || 'dark';
      const color = localStorage.getItem('bloom-theme-color') || '#007aff';
      const opacity = parseFloat(localStorage.getItem('bloom-theme-opacity') || '0.80');
      const brightness = parseFloat(localStorage.getItem('bloom-theme-brightness') || '0.15');
      applyTheme(mode, color, opacity, Number(value), brightness);
    }
    if (key === 'theme-brightness') {
      localStorage.setItem('bloom-theme-brightness', String(value));
      const mode = localStorage.getItem('bloom-theme-mode') || 'dark';
      const color = localStorage.getItem('bloom-theme-color') || '#007aff';
      const opacity = parseFloat(localStorage.getItem('bloom-theme-opacity') || '0.80');
      const saturation = parseFloat(localStorage.getItem('bloom-theme-saturation') || '0.50');
      applyTheme(mode, color, opacity, saturation, Number(value));
    }
  });

  // Listen to system accent color updates in case of adaptive theme
  const accentPromise = listen<string>('system-accent-changed', (event) => {
    const currentMode = localStorage.getItem('bloom-theme-mode') || 'dark';
    if (currentMode === 'adaptive') {
      const opacity = parseFloat(localStorage.getItem('bloom-theme-opacity') || '0.80');
      const saturation = parseFloat(localStorage.getItem('bloom-theme-saturation') || '0.50');
      const brightness = parseFloat(localStorage.getItem('bloom-theme-brightness') || '0.15');
      applyTheme('adaptive', event.payload, opacity, saturation, brightness);
    }
  });

  return () => {
    settingsPromise.then(f => f());
    accentPromise.then(f => f());
  };
}
