//! Share links: `<site>?world=<code>`, where the code comes from
//! `worldgen_core::share`. The web build reads the code on startup and keeps
//! the address bar pointing at the current world.

use worldgen_core::{share, WorldConfig};

/// Where the web build is published; native builds link here.
pub const SITE_URL: &str = "https://dmadan54.github.io/procedural-world-generator/";

/// The `world` parameter from a query string or full URL, if present.
pub fn code_from_query(text: &str) -> Option<&str> {
    let query = text.split_once('?').map_or(text, |(_, q)| q);
    let query = query.split('#').next().unwrap_or_default();
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("world="))
        .filter(|code| !code.is_empty())
}

/// The config a share link (or bare code) describes.
pub fn config_from_link(text: &str) -> Result<WorldConfig, String> {
    let code = code_from_query(text).unwrap_or(text);
    share::decode(code).map_err(|e| e.to_string())
}

/// The link that opens `config`.
pub fn link_for(config: &WorldConfig) -> String {
    format!("{}?world={}", base_url(), share::encode(config))
}

/// The config to start with: from the page URL on the web, or from a
/// `--world <link or code>` argument natively. Falls back to the default,
/// with an error message if a link was given but could not be read.
pub fn startup_config() -> (WorldConfig, Option<String>) {
    match startup_link() {
        None => (WorldConfig::default(), None),
        Some(link) => match config_from_link(&link) {
            Ok(config) => (config, None),
            Err(e) => (
                WorldConfig::default(),
                Some(format!("Couldn't open the shared world: {e}")),
            ),
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn startup_link() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    code_from_query(&search).map(str::to_owned)
}

#[cfg(not(target_arch = "wasm32"))]
fn startup_link() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--world" {
            return args.next();
        }
        if let Some(value) = arg.strip_prefix("--world=") {
            return Some(value.to_owned());
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn base_url() -> String {
    web_sys::window()
        .and_then(|w| {
            let location = w.location();
            Some(format!(
                "{}{}",
                location.origin().ok()?,
                location.pathname().ok()?
            ))
        })
        .unwrap_or_else(|| SITE_URL.to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn base_url() -> String {
    SITE_URL.to_owned()
}

/// Points the browser's address bar at `config` without reloading, so the
/// URL can be copied straight from it. Does nothing natively.
pub fn show_in_address_bar(config: &WorldConfig) {
    #[cfg(target_arch = "wasm32")]
    if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
        let url = format!("?world={}", share::encode(config));
        let _ =
            history.replace_state_with_url(&eframe::wasm_bindgen::JsValue::NULL, "", Some(&url));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = config;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_world_parameter() {
        assert_eq!(code_from_query("?world=abc"), Some("abc"));
        assert_eq!(
            code_from_query("https://x.io/p/?a=1&world=abc#top"),
            Some("abc")
        );
        assert_eq!(code_from_query("?a=1"), None);
        assert_eq!(code_from_query("?world="), None);
    }

    #[test]
    fn links_round_trip() {
        let mut config = WorldConfig {
            seed: 4242,
            ..Default::default()
        };
        config.terrain.sea_level = 0.5;
        let link = link_for(&config);
        assert!(link.starts_with(SITE_URL));
        assert_eq!(config_from_link(&link).unwrap(), config);
    }

    #[test]
    fn bare_codes_work_too() {
        let config = WorldConfig {
            seed: 9,
            ..Default::default()
        };
        assert_eq!(config_from_link(&share::encode(&config)).unwrap(), config);
    }
}
