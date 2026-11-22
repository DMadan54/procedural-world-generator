//! Compact, URL-safe share codes for a [`WorldConfig`].
//!
//! A code holds the seed plus only the settings that differ from the
//! defaults, as JSON, encoded with unpadded URL-safe base64. Fields missing
//! from a code take their defaults when decoded, so codes stay valid when new
//! settings are added. A typical code with a handful of changes is well under
//! a hundred characters.

use serde_json::{Map, Number, Value};

use crate::config::WorldConfig;

#[derive(Debug, PartialEq, Eq)]
pub enum ShareError {
    NotBase64,
    NotJson(String),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareError::NotBase64 => write!(f, "the world code is not valid base64"),
            ShareError::NotJson(e) => write!(f, "the world code does not describe a world: {e}"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Encodes a config as a share code.
pub fn encode(config: &WorldConfig) -> String {
    let value = serde_json::to_value(config).expect("configs always serialise");
    let defaults = serde_json::to_value(WorldConfig::default()).expect("configs always serialise");
    let mut diff = match difference(&value, &defaults) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    // Always carry the seed, so a code names one world even if defaults change.
    diff.insert("seed".into(), Value::from(config.seed));
    let json = serde_json::to_string(&Value::Object(diff)).expect("values always serialise");
    base64_encode(json.as_bytes())
}

/// Decodes a share code back into a full config.
pub fn decode(code: &str) -> Result<WorldConfig, ShareError> {
    let bytes = base64_decode(code.trim()).ok_or(ShareError::NotBase64)?;
    let text = String::from_utf8(bytes).map_err(|e| ShareError::NotJson(e.to_string()))?;
    from_partial_json(&text)
}

/// Reads a config from JSON that may name only some settings; everything
/// else takes the value from `WorldConfig::default()`.
///
/// Overlaying onto the full default config matters for nested sections:
/// deserialising a partial section on its own would fill gaps from that
/// section type's `Default`, which can differ from the world default (each
/// disaster has its own default frequency, for example).
pub fn from_partial_json(json: &str) -> Result<WorldConfig, ShareError> {
    let patch: Value =
        serde_json::from_str(json).map_err(|e| ShareError::NotJson(e.to_string()))?;
    let mut full = serde_json::to_value(WorldConfig::default()).expect("configs always serialise");
    overlay(&mut full, &patch);
    serde_json::from_value(full).map_err(|e| ShareError::NotJson(e.to_string()))
}

fn overlay(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                match base.get_mut(key) {
                    Some(existing) => overlay(existing, value),
                    None => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

/// The parts of `value` that differ from `base`, or `None` if equal.
fn difference(value: &Value, base: &Value) -> Option<Value> {
    match (value, base) {
        (Value::Object(v), Value::Object(b)) => {
            let mut out = Map::new();
            for (key, field) in v {
                let changed = match b.get(key) {
                    Some(base_field) => difference(field, base_field),
                    None => Some(compact(field)),
                };
                if let Some(changed) = changed {
                    out.insert(key.clone(), changed);
                }
            }
            (!out.is_empty()).then_some(Value::Object(out))
        }
        _ if value == base => None,
        _ => Some(compact(value)),
    }
}

/// Shortens floats that came from `f32` fields: they serialise as their exact
/// `f64` widening (0.44999998807907104) but round-trip from the shortest `f32`
/// form (0.45).
fn compact(value: &Value) -> Value {
    match value {
        Value::Number(n) if n.is_f64() => {
            let v = n.as_f64().unwrap_or_default();
            let narrow = v as f32;
            if narrow as f64 == v {
                if let Ok(short) = narrow.to_string().parse::<f64>() {
                    if let Some(number) = Number::from_f64(short) {
                        return Value::Number(number);
                    }
                }
            }
            value.clone()
        }
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), compact(v))).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(compact).collect()),
        other => other.clone(),
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for k in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|p| p as u32);
    let chars = text.trim_end_matches('=').as_bytes();
    if chars.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(chars.len() * 3 / 4);
    for chunk in chars.chunks(4) {
        let mut n = 0u32;
        for (k, &c) in chunk.iter().enumerate() {
            n |= digit(c)? << (18 - 6 * k);
        }
        for k in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MagicLevel;

    #[test]
    fn default_world_round_trips() {
        let config = WorldConfig::default();
        assert_eq!(decode(&encode(&config)).unwrap(), config);
    }

    #[test]
    fn changed_world_round_trips_exactly() {
        let mut config = WorldConfig {
            seed: 987_654_321_012,
            ..Default::default()
        };
        config.terrain.sea_level = 0.53;
        config.tectonics.plate_count = 31;
        config.tectonics.boundary_warp = 0.0712345;
        config.climate.global_temperature_offset = -7.5;
        config.magic.level = MagicLevel::Saturated;
        config.disasters.plagues.enabled = false;
        config.history.years = 12_000;
        assert_eq!(decode(&encode(&config)).unwrap(), config);
    }

    #[test]
    fn codes_are_short_and_url_safe() {
        let mut config = WorldConfig {
            seed: 123_456,
            ..Default::default()
        };
        config.terrain.sea_level = 0.5;
        config.magic.level = MagicLevel::Rare;
        let code = encode(&config);
        assert!(code.len() < 100, "{code} is {} chars", code.len());
        assert!(code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let code = base64_encode(br#"{"seed":7}"#);
        let config = decode(&code).unwrap();
        assert_eq!(
            config,
            WorldConfig {
                seed: 7,
                ..Default::default()
            }
        );
    }

    #[test]
    fn partial_sections_keep_world_defaults() {
        let config = from_partial_json(r#"{"disasters":{"plagues":{"enabled":false}}}"#).unwrap();
        let defaults = WorldConfig::default();
        assert!(!config.disasters.plagues.enabled);
        assert_eq!(
            config.disasters.plagues.per_century,
            defaults.disasters.plagues.per_century
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(decode("!!!"), Err(ShareError::NotBase64));
        assert!(matches!(
            decode(&base64_encode(b"not json")),
            Err(ShareError::NotJson(_))
        ));
    }

    #[test]
    fn base64_round_trips_every_length() {
        for len in 0..20 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
        }
    }
}
