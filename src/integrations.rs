use std::{
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};

const WORD_OF_DAY_FEED_URL: &str = "https://www.merriam-webster.com/wotd/feed/rss2";
pub(crate) const WORD_OF_DAY_SOURCE: &str = "Merriam-Webster";
const WEATHER_GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const WEATHER_FALLBACK_GEOCODING_URL: &str = "https://nominatim.openstreetmap.org/search";
const WEATHER_FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
pub(crate) const WEATHER_SOURCE: &str = "Open-Meteo";
const WEATHER_FALLBACK_SOURCE: &str = "OpenStreetMap Nominatim";
const USER_AGENT: &str = "Everything Starter";

#[derive(Clone, Debug)]
pub(crate) struct WordOfDay {
    pub(crate) word: String,
    pub(crate) definition: String,
    pub(crate) published: String,
    pub(crate) link: String,
}

#[derive(Clone, Debug)]
pub(crate) enum WordOfDayState {
    Loading,
    Ready(WordOfDay),
    Error(String),
}

#[derive(Clone, Debug)]
pub(crate) struct WeatherReport {
    pub(crate) location: String,
    pub(crate) condition: String,
    pub(crate) temperature_c: f64,
    pub(crate) apparent_c: Option<f64>,
    pub(crate) humidity: Option<u64>,
    pub(crate) precipitation_mm: Option<f64>,
    pub(crate) wind_kmh: Option<f64>,
    pub(crate) high_c: Option<f64>,
    pub(crate) low_c: Option<f64>,
    pub(crate) precipitation_probability: Option<u64>,
    pub(crate) updated_at: String,
}

#[derive(Clone, Debug)]
pub(crate) struct WeatherLocation {
    pub(crate) latitude: f64,
    pub(crate) longitude: f64,
    pub(crate) label: String,
}

#[derive(Clone, Debug)]
pub(crate) enum WeatherState {
    Idle,
    Loading,
    Ready(WeatherReport),
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum WeatherUnits {
    #[default]
    Metric,
    Imperial,
}

pub(crate) struct WordOfDayFeature {
    pub(crate) state: WordOfDayState,
    pub(crate) receiver: Option<Receiver<Result<WordOfDay, String>>>,
    pub(crate) requested_date: String,
}

impl WordOfDayFeature {
    pub(crate) fn new(
        receiver: Option<Receiver<Result<WordOfDay, String>>>,
        requested_date: String,
    ) -> Self {
        Self {
            state: WordOfDayState::Loading,
            receiver,
            requested_date,
        }
    }
}

pub(crate) struct WeatherFeature {
    pub(crate) location_input: String,
    pub(crate) state: WeatherState,
    pub(crate) receiver: Option<Receiver<Result<WeatherReport, String>>>,
    pub(crate) requested_location: String,
    pub(crate) units: WeatherUnits,
    pub(crate) settings_open: bool,
}

impl WeatherFeature {
    pub(crate) fn new(
        location_input: String,
        state: WeatherState,
        receiver: Option<Receiver<Result<WeatherReport, String>>>,
        requested_location: String,
        units: WeatherUnits,
    ) -> Self {
        Self {
            location_input,
            state,
            receiver,
            requested_location,
            units,
            settings_open: false,
        }
    }
}

pub(crate) fn spawn_word_of_day_fetch() -> Receiver<Result<WordOfDay, String>> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = fetch_word_of_day();
        let _ = sender.send(result);
    });

    receiver
}

pub(crate) fn fetch_word_of_day() -> Result<WordOfDay, String> {
    let response = ureq::get(WORD_OF_DAY_FEED_URL)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| format!("Could not fetch {WORD_OF_DAY_SOURCE} feed: {error}"))?;
    let body = response
        .into_string()
        .map_err(|error| format!("Could not read {WORD_OF_DAY_SOURCE} feed: {error}"))?;

    parse_word_of_day_feed(&body)
}

pub(crate) fn parse_word_of_day_feed(feed: &str) -> Result<WordOfDay, String> {
    let document = roxmltree::Document::parse(feed)
        .map_err(|error| format!("Could not parse {WORD_OF_DAY_SOURCE} feed: {error}"))?;
    let item = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "item")
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include an item."))?;

    let word = child_text_by_name(item, "title")
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include a word."))?;
    let definition = child_text_by_name(item, "shortdef")
        .or_else(|| {
            child_text_by_name(item, "summary")
                .and_then(|summary| definition_from_summary(&summary, &word))
        })
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{WORD_OF_DAY_SOURCE} feed did not include a definition."))?;
    let published = child_text_by_name(item, "pubDate").unwrap_or_default();
    let link = child_text_by_name(item, "link")
        .unwrap_or_else(|| "https://www.merriam-webster.com/word-of-the-day".to_owned());

    Ok(WordOfDay {
        word,
        definition,
        published,
        link,
    })
}

pub(crate) fn child_text_by_name(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(clean_feed_text)
}

pub(crate) fn definition_from_summary(summary: &str, word: &str) -> Option<String> {
    let word_index = summary.find(word)?;
    let after_word = summary[word_index + word.len()..].trim();
    let definition_start = after_word.find("  ")?;
    let after_heading = after_word[definition_start..].trim();
    after_heading
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('['))
        .map(clean_feed_text)
}

pub(crate) fn clean_feed_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn spawn_weather_fetch(location: String) -> Receiver<Result<WeatherReport, String>> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = fetch_weather(&location);
        let _ = sender.send(result);
    });

    receiver
}

pub(crate) fn fetch_weather(location: &str) -> Result<WeatherReport, String> {
    let location = normalize_title(location);
    if location.is_empty() {
        return Err("Enter a location first.".to_owned());
    }

    let resolved_location = find_weather_location(&location)?;

    let forecast_url = format!(
        "{WEATHER_FORECAST_URL}?latitude={}&longitude={}&current=temperature_2m,relative_humidity_2m,apparent_temperature,precipitation,weather_code,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone=auto&forecast_days=1",
        resolved_location.latitude, resolved_location.longitude
    );
    let forecast_body = fetch_text(&forecast_url, WEATHER_SOURCE)?;
    let forecast_json: serde_json::Value = serde_json::from_str(&forecast_body)
        .map_err(|error| format!("Could not parse weather forecast: {error}"))?;
    let current = forecast_json
        .get("current")
        .ok_or_else(|| "Weather forecast did not include current conditions.".to_owned())?;
    let daily = forecast_json.get("daily");

    let temperature_c = required_json_f64(current, "temperature_2m")?;
    let weather_code = optional_json_i64(current, "weather_code");

    Ok(WeatherReport {
        location: resolved_location.label,
        condition: weather_code
            .map(weather_code_label)
            .unwrap_or("Current conditions".to_owned()),
        temperature_c,
        apparent_c: optional_json_f64(current, "apparent_temperature"),
        humidity: optional_json_u64(current, "relative_humidity_2m"),
        precipitation_mm: optional_json_f64(current, "precipitation"),
        wind_kmh: optional_json_f64(current, "wind_speed_10m"),
        high_c: daily.and_then(|daily| first_json_array_f64(daily, "temperature_2m_max")),
        low_c: daily.and_then(|daily| first_json_array_f64(daily, "temperature_2m_min")),
        precipitation_probability: daily
            .and_then(|daily| first_json_array_u64(daily, "precipitation_probability_max")),
        updated_at: current
            .get("time")
            .and_then(serde_json::Value::as_str)
            .map_or_else(String::new, ToOwned::to_owned),
    })
}

fn normalize_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn fetch_text(url: &str, source: &str) -> Result<String, String> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(8))
        .call()
        .map_err(|error| format!("Could not fetch {source}: {error}"))?;

    response
        .into_string()
        .map_err(|error| format!("Could not read {source} response: {error}"))
}

pub(crate) fn find_weather_location(location: &str) -> Result<WeatherLocation, String> {
    find_weather_location_with(
        location,
        find_open_meteo_location,
        find_fallback_weather_location,
    )
}

fn find_weather_location_with<Primary, Fallback>(
    location: &str,
    primary: Primary,
    fallback: Fallback,
) -> Result<WeatherLocation, String>
where
    Primary: FnOnce(&str) -> Result<Option<WeatherLocation>, String>,
    Fallback: FnOnce(&str) -> Result<Option<WeatherLocation>, String>,
{
    let primary_error = match primary(location) {
        Ok(Some(location_match)) => return Ok(location_match),
        Ok(None) => None,
        Err(error) => Some(error),
    };

    match fallback(location) {
        Ok(Some(location_match)) => Ok(location_match),
        Ok(None) => match primary_error {
            Some(error) => Err(format!(
                "Primary location lookup failed ({error}); the fallback found no location for {location}."
            )),
            None => Err(format!("No weather location found for {location}.")),
        },
        Err(fallback_error) => match primary_error {
            Some(primary_error) => Err(format!(
                "Location services failed. Primary: {primary_error}. Fallback: {fallback_error}"
            )),
            None => Err(fallback_error),
        },
    }
}

pub(crate) fn find_open_meteo_location(location: &str) -> Result<Option<WeatherLocation>, String> {
    let geocoding_url = format!(
        "{WEATHER_GEOCODING_URL}?name={}&count=1&language=en&format=json",
        percent_encode_query_value(location)
    );
    let geocoding_body = fetch_text(&geocoding_url, WEATHER_SOURCE)?;
    let geocoding_json: serde_json::Value = serde_json::from_str(&geocoding_body)
        .map_err(|error| format!("Could not parse location search: {error}"))?;
    let Some(result) = geocoding_json
        .get("results")
        .and_then(serde_json::Value::as_array)
        .and_then(|results| results.first())
    else {
        return Ok(None);
    };

    Ok(Some(WeatherLocation {
        latitude: required_json_f64(result, "latitude")?,
        longitude: required_json_f64(result, "longitude")?,
        label: open_meteo_location_label(result),
    }))
}

pub(crate) fn find_fallback_weather_location(
    location: &str,
) -> Result<Option<WeatherLocation>, String> {
    let geocoding_url = format!(
        "{WEATHER_FALLBACK_GEOCODING_URL}?q={}&format=jsonv2&limit=1",
        percent_encode_query_value(location)
    );
    let geocoding_body = fetch_text(&geocoding_url, WEATHER_FALLBACK_SOURCE)?;
    let geocoding_json: serde_json::Value = serde_json::from_str(&geocoding_body)
        .map_err(|error| format!("Could not parse fallback location search: {error}"))?;
    let Some(result) = geocoding_json
        .as_array()
        .and_then(|results| results.first())
    else {
        return Ok(None);
    };

    Ok(Some(WeatherLocation {
        latitude: required_json_str_f64(result, "lat")?,
        longitude: required_json_str_f64(result, "lon")?,
        label: fallback_weather_location_label(result),
    }))
}

pub(crate) fn open_meteo_location_label(result: &serde_json::Value) -> String {
    let name = result
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Selected location");
    let admin = result.get("admin1").and_then(serde_json::Value::as_str);
    let country = result.get("country").and_then(serde_json::Value::as_str);
    [Some(name), admin, country]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn fallback_weather_location_label(result: &serde_json::Value) -> String {
    result
        .get("display_name")
        .and_then(serde_json::Value::as_str)
        .or_else(|| result.get("name").and_then(serde_json::Value::as_str))
        .unwrap_or("Selected location")
        .to_owned()
}

pub(crate) fn required_json_f64(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    optional_json_f64(value, key).ok_or_else(|| format!("Weather response was missing {key}."))
}

pub(crate) fn required_json_str_f64(value: &serde_json::Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("Location response was missing {key}."))?
        .parse::<f64>()
        .map_err(|error| format!("Location response had invalid {key}: {error}"))
}

pub(crate) fn optional_json_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(serde_json::Value::as_f64)
}

pub(crate) fn optional_json_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

pub(crate) fn optional_json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(serde_json::Value::as_u64)
}

pub(crate) fn first_json_array_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_f64)
}

pub(crate) fn first_json_array_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_array)
        .and_then(|values| values.first())
        .and_then(serde_json::Value::as_u64)
}

pub(crate) fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn weather_code_label(code: i64) -> String {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 | 63 | 65 => "Rain",
        66 | 67 => "Freezing rain",
        71 | 73 | 75 => "Snow",
        77 => "Snow grains",
        80..=82 => "Rain showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Current conditions",
    }
    .to_owned()
}

pub(crate) fn c_to_f(celsius: f64) -> f64 {
    celsius * 9.0 / 5.0 + 32.0
}

pub(crate) fn format_temperature(celsius: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{celsius:.0}\u{00B0}C"),
        WeatherUnits::Imperial => format!("{:.0}\u{00B0}F", c_to_f(celsius)),
    }
}

pub(crate) fn format_temperature_short(celsius: f64, units: WeatherUnits) -> String {
    format_temperature(celsius, units)
}

pub(crate) fn format_wind(kmh: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{kmh:.0} km/h"),
        WeatherUnits::Imperial => format!("{:.0} mph", kmh * 0.621_371),
    }
}

pub(crate) fn format_precipitation(mm: f64, units: WeatherUnits) -> String {
    match units {
        WeatherUnits::Metric => format!("{mm:.1} mm"),
        WeatherUnits::Imperial => format!("{:.2} in", mm / 25.4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_location_falls_back_when_primary_lookup_fails() {
        let resolved = find_weather_location_with(
            "St Albans",
            |_| Err("primary service unavailable".to_owned()),
            |_| {
                Ok(Some(WeatherLocation {
                    latitude: 51.75,
                    longitude: -0.33,
                    label: "St Albans, England".to_owned(),
                }))
            },
        )
        .expect("fallback location should be used");

        assert_eq!(resolved.label, "St Albans, England");
        assert_eq!(resolved.latitude, 51.75);
        assert_eq!(resolved.longitude, -0.33);
    }
}
