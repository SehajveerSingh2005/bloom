//! get_weather: current conditions and the forecast from Open-Meteo (free, no
//! key), the same source and location fallback Bloom's own widget uses.

use crate::agent::Ctx;
use serde_json::Value;

/// Base URLs, swapped for a local server in tests.
pub struct Endpoints {
    pub forecast: String,
    pub geocode: String,
    pub ip_primary: String,
    pub ip_fallback: String,
}

impl Default for Endpoints {
    fn default() -> Endpoints {
        Endpoints {
            forecast: "https://api.open-meteo.com/v1/forecast".into(),
            geocode: "https://geocoding-api.open-meteo.com/v1/search".into(),
            ip_primary: "https://ipapi.co/json/".into(),
            // ip-api.com's free tier is HTTP only (as in Bloom's useWeather.ts).
            ip_fallback: "http://ip-api.com/json/?fields=status,lat,lon,city".into(),
        }
    }
}

struct Place {
    lat: f64,
    lon: f64,
    city: String,
}

/// WMO weather interpretation codes, as Bloom's widget names them.
fn condition(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Foggy",
        51..=55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61..=65 => "Rain",
        66 | 67 => "Freezing rain",
        71..=77 => "Snow",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95..=99 => "Thunderstorm",
        _ => "Unknown",
    }
}

async fn json(ctx: &Ctx, url: &str, params: &[(&str, &str)]) -> Result<Value, String> {
    let url = reqwest::Url::parse_with_params(url, params).map_err(|e| e.to_string())?;
    let res = ctx
        .shared
        .http
        .get(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("Can't reach the weather service: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("Weather service error ({})", res.status()));
    }
    res.json()
        .await
        .map_err(|_| "The weather service sent something unreadable.".to_string())
}

async fn geocode(ctx: &Ctx, city: &str) -> Result<Place, String> {
    let found = json(
        ctx,
        &ctx.shared.endpoints.geocode,
        &[("name", city), ("count", "1")],
    )
    .await?;
    let r = &found["results"][0];
    match (r["latitude"].as_f64(), r["longitude"].as_f64()) {
        (Some(lat), Some(lon)) => Ok(Place {
            lat,
            lon,
            city: r["name"].as_str().unwrap_or(city).to_string(),
        }),
        _ => Err(format!("No place called {city} was found.")),
    }
}

/// An explicit city, else the coordinates saved in Bloom's settings, else the
/// saved city name, else a lookup by IP address.
async fn locate(ctx: &Ctx, city: Option<&str>) -> Result<Place, String> {
    if let Some(city) = city.map(str::trim).filter(|c| !c.is_empty()) {
        return geocode(ctx, city).await;
    }
    let cfg = &ctx.cfg;
    if let (Some(lat), Some(lon)) = (cfg.weather_lat, cfg.weather_lon) {
        return Ok(Place {
            lat,
            lon,
            city: cfg.weather_city.clone(),
        });
    }
    if !cfg.weather_city.is_empty() {
        // A saved name that won't geocode falls through to the IP lookup.
        if let Ok(place) = geocode(ctx, &cfg.weather_city).await {
            return Ok(place);
        }
    }
    let e = &ctx.shared.endpoints;
    for url in [&e.ip_primary, &e.ip_fallback] {
        if let Ok(d) = json(ctx, url, &[]).await {
            // ipapi.co says latitude/longitude, ip-api.com says lat/lon.
            let coord = |a: &str, b: &str| d[a].as_f64().or_else(|| d[b].as_f64());
            if let (Some(la), Some(lo)) = (coord("latitude", "lat"), coord("longitude", "lon")) {
                let city = d["city"].as_str().unwrap_or_default().to_string();
                return Ok(Place {
                    lat: la,
                    lon: lo,
                    city,
                });
            }
        }
    }
    Err(
        "Couldn't work out where the user is. Ask which city, then call get_weather with city."
            .into(),
    )
}

pub async fn get(ctx: &Ctx, city: Option<&str>) -> Result<String, String> {
    let place = locate(ctx, city).await?;
    let fahrenheit = ctx.cfg.fahrenheit;
    let (lat, lon) = (place.lat.to_string(), place.lon.to_string());
    let mut params = vec![
        ("latitude", lat.as_str()),
        ("longitude", lon.as_str()),
        (
            "current",
            "temperature_2m,apparent_temperature,weather_code,wind_speed_10m",
        ),
        (
            "daily",
            "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
        ),
        ("forecast_days", "2"),
        ("timezone", "auto"),
    ];
    if fahrenheit {
        params.push(("temperature_unit", "fahrenheit"));
    }
    let d = json(ctx, &ctx.shared.endpoints.forecast, &params).await?;
    summary(&d, &place.city, if fahrenheit { "F" } else { "C" })
}

fn summary(d: &Value, city: &str, unit: &str) -> Result<String, String> {
    let n = |v: &Value| v.as_f64().map(|x| x.round() as i64);
    let cur = &d["current"];
    let Some(now) = n(&cur["temperature_2m"]) else {
        return Err("The weather service sent no data.".into());
    };
    let place = if city.is_empty() {
        String::new()
    } else {
        format!("{city}. ")
    };
    let mut out = format!(
        "{place}Now: {now}{unit}, feels like {}{unit}, {}, wind {} km/h.",
        n(&cur["apparent_temperature"]).unwrap_or(now),
        condition(cur["weather_code"].as_i64().unwrap_or(-1)),
        n(&cur["wind_speed_10m"]).unwrap_or(0),
    );
    let daily = &d["daily"];
    for (i, label) in ["Today", "Tomorrow"].iter().enumerate() {
        if let (Some(hi), Some(lo)) = (
            n(&daily["temperature_2m_max"][i]),
            n(&daily["temperature_2m_min"][i]),
        ) {
            out += &format!(
                " {label}: {}, high {hi}{unit}, low {lo}{unit}",
                condition(daily["weather_code"][i].as_i64().unwrap_or(-1))
            );
            if let Some(p) = n(&daily["precipitation_probability_max"][i]) {
                out += &format!(", {p}% chance of rain");
            }
            out.push('.');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{ctx_with_endpoints, mock_server};

    const FORECAST: &str = r#"{"current":{"temperature_2m":30.6,"apparent_temperature":33.2,"weather_code":2,"wind_speed_10m":11.4},"daily":{"weather_code":[61,0],"temperature_2m_max":[35.1,33.0],"temperature_2m_min":[26.0,25.4],"precipitation_probability_max":[40,5]}}"#;

    #[tokio::test]
    async fn saved_coordinates_skip_lookups_and_units_follow_the_setting() {
        let (url, _r) = mock_server(vec![FORECAST.into(), FORECAST.into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_lat = Some(28.6);
        ctx.cfg.weather_lon = Some(77.2);
        ctx.cfg.weather_city = "Delhi".into();
        assert_eq!(
            get(&ctx, None).await.unwrap(),
            "Delhi. Now: 31C, feels like 33C, Partly cloudy, wind 11 km/h. \
             Today: Rain, high 35C, low 26C, 40% chance of rain. \
             Tomorrow: Clear, high 33C, low 25C, 5% chance of rain."
        );
        ctx.cfg.fahrenheit = true;
        assert!(get(&ctx, None).await.unwrap().contains("Now: 31F"));
    }

    #[tokio::test]
    async fn city_name_is_geocoded_when_there_are_no_coordinates() {
        let geo = r#"{"results":[{"name":"Pune","latitude":18.5,"longitude":73.8}]}"#;
        let (url, _r) = mock_server(vec![geo.into(), FORECAST.into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_city = "pune".into();
        assert!(get(&ctx, None).await.unwrap().starts_with("Pune. Now"));
    }

    #[tokio::test]
    async fn falls_back_to_ip_lookup_then_to_the_second_service() {
        let first_fails = r#"{"error":true}"#;
        let second = r#"{"status":"success","lat":19.0,"lon":72.8,"city":"Mumbai"}"#;
        let (url, _r) = mock_server(vec![first_fails.into(), second.into(), FORECAST.into()]);
        let ctx = ctx_with_endpoints(&url);
        assert!(get(&ctx, None).await.unwrap().starts_with("Mumbai. Now"));
    }

    #[tokio::test]
    async fn unknown_saved_city_falls_through_to_ip_lookup() {
        let none = r#"{}"#;
        let ip = r#"{"lat":19.0,"lon":72.8,"city":"Mumbai"}"#;
        let (url, _r) = mock_server(vec![none.into(), ip.into(), FORECAST.into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_city = "Nowhereville".into();
        assert!(get(&ctx, None).await.unwrap().starts_with("Mumbai. Now"));
    }

    #[tokio::test]
    async fn service_errors_and_garbage_give_clean_messages() {
        let (url, _r) =
            crate::testutil::mock_server_status("500 Internal Server Error", vec!["x".into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_lat = Some(1.0);
        ctx.cfg.weather_lon = Some(1.0);
        assert_eq!(
            get(&ctx, None).await,
            Err("Weather service error (500 Internal Server Error)".into())
        );
        let (url, _r) = mock_server(vec!["<html>nope".into()]);
        let ctx = {
            let mut c = ctx_with_endpoints(&url);
            c.cfg.weather_lat = Some(1.0);
            c.cfg.weather_lon = Some(1.0);
            c
        };
        assert_eq!(
            get(&ctx, None).await,
            Err("The weather service sent something unreadable.".into())
        );
    }

    #[tokio::test]
    async fn explicit_city_wins_over_saved_location() {
        let geo = r#"{"results":[{"name":"Paris","latitude":48.8,"longitude":2.3}]}"#;
        let (url, _r) = mock_server(vec![geo.into(), FORECAST.into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_lat = Some(1.0);
        ctx.cfg.weather_lon = Some(1.0);
        assert!(get(&ctx, Some("Paris"))
            .await
            .unwrap()
            .starts_with("Paris. Now"));
    }

    #[tokio::test]
    async fn empty_response_is_an_error() {
        let (url, _r) = mock_server(vec!["{}".into()]);
        let mut ctx = ctx_with_endpoints(&url);
        ctx.cfg.weather_lat = Some(1.0);
        ctx.cfg.weather_lon = Some(1.0);
        assert!(get(&ctx, None).await.is_err());
    }
}
