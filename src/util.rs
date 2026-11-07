//! Shared parsing, formatting and name-validation helpers.

use serde::Deserialize;

pub(crate) fn validate_name<'a>(value: &'a str, what: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{what} name cannot be empty"));
    }
    if value.starts_with('-') {
        return Err(format!("{what} name cannot start with '-'"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(format!(
            "{what} names may only contain letters, digits, '-', '_' and '.'"
        ));
    }
    Ok(value)
}

pub(crate) fn display_created(raw: &str) -> String {
    if raw.is_empty() {
        "-".to_string()
    } else {
        format_timestamp(Some(raw))
    }
}

pub(crate) fn parse_json<T: for<'de> Deserialize<'de>>(
    json: &str,
    command: &str,
) -> Result<Vec<T>, String> {
    let trimmed = json.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(trimmed)
        .map_err(|err| format!("could not parse `{command}` output: {err}"))
}

pub(crate) fn first_non_empty(values: Option<&[String]>) -> Option<String> {
    values?
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
}

pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

pub(crate) fn format_timestamp(raw: Option<&str>) -> String {
    match raw {
        None => "-".to_string(),
        Some(raw) if raw.len() >= 16 => raw[..16].replace('T', " "),
        Some(raw) => raw.to_string(),
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Parse values such as `1.712MB`, `556.2kB`, `16.38GB` or `0B`.
pub(crate) fn parse_size(value: &str) -> u64 {
    let value = value.trim();
    let split = value
        .find(|character: char| {
            !(character.is_ascii_digit()
                || character == '.'
                || character == '+'
                || character == '-')
        })
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().unwrap_or_default();
    let unit = unit.trim();
    let multiplier = match unit {
        "" | "B" => 1.0,
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "PB" => 1e15,
        "KiB" => 1024.0,
        "MiB" => 1_048_576.0,
        "GiB" => 1_073_741_824.0,
        "TiB" => 1_099_511_627_776.0,
        _ => 1.0,
    };
    (number * multiplier).round() as u64
}

/// Podman reports some numeric fields as JSON strings on some versions
/// (`"Memory": "2147483648"`, `"DiskSize": "107374182400"`) and as numbers on
/// others, so accept both.
pub(crate) fn lenient_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberOrText {
        Number(u64),
        Text(String),
    }

    Ok(match Option::<NumberOrText>::deserialize(deserializer)? {
        None => None,
        Some(NumberOrText::Number(value)) => Some(value),
        Some(NumberOrText::Text(text)) => text.trim().parse().ok(),
    })
}
