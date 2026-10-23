//! Which theme herdr is using, so the phone can match it.
//!
//! Read from herdr's own config file on each phone connection (cheap, and a
//! theme change shows up on the next reconnect). Paths and theme names mirror
//! herdr's `config/io.rs` and `config/theme.rs`.

use std::path::PathBuf;

use shepsky_core::protocol::HerdrTheme;
use serde::Deserialize;

/// herdr's default theme (also its fallback for unknown names).
const DEFAULT_THEME: &str = "catppuccin";
const DEFAULT_LIGHT_THEME: &str = "catppuccin-latte";

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    theme: ThemeConfig,
}

#[derive(Debug, Default, Deserialize)]
struct ThemeConfig {
    name: Option<String>,
    #[serde(default)]
    auto_switch: bool,
    dark_name: Option<String>,
    light_name: Option<String>,
}

/// `$HERDR_CONFIG_PATH`, else `$XDG_CONFIG_HOME/herdr/config.toml`, else
/// `~/.config/herdr/config.toml`.
fn config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HERDR_CONFIG_PATH") {
        return Some(PathBuf::from(path));
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(dir).join("herdr/config.toml"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/herdr/config.toml"))
}

/// herdr's theme, falling back to herdr's defaults when the file or the
/// `[theme]` table is missing or unreadable.
pub fn load() -> HerdrTheme {
    let config = config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| match toml::from_str::<Config>(&text) {
            Ok(config) => Some(config),
            Err(err) => {
                tracing::debug!(%err, "herdr config unreadable; using herdr's default theme");
                None
            }
        })
        .unwrap_or_default();
    resolve(&config.theme)
}

fn resolve(theme: &ThemeConfig) -> HerdrTheme {
    let pick = |name: Option<&str>, fallback: &'static str| {
        name.and_then(canonical_name).unwrap_or(fallback).to_string()
    };
    let name = pick(theme.name.as_deref(), DEFAULT_THEME);
    if theme.auto_switch {
        HerdrTheme {
            name,
            dark: Some(pick(theme.dark_name.as_deref(), DEFAULT_THEME)),
            light: Some(pick(theme.light_name.as_deref(), DEFAULT_LIGHT_THEME)),
        }
    } else {
        HerdrTheme { name, dark: None, light: None }
    }
}

/// herdr's built-in theme names, with the aliases herdr accepts.
fn canonical_name(name: &str) -> Option<&'static str> {
    Some(match name.to_lowercase().replace([' ', '_'], "-").as_str() {
        "catppuccin" | "catppuccin-mocha" => "catppuccin",
        "catppuccin-latte" | "latte" | "light" => "catppuccin-latte",
        "terminal" => "terminal",
        "tokyo-night" | "tokyonight" => "tokyo-night",
        "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => "tokyo-night-day",
        "dracula" => "dracula",
        "nord" => "nord",
        "gruvbox" | "gruvbox-dark" => "gruvbox",
        "gruvbox-light" => "gruvbox-light",
        "one-dark" | "onedark" => "one-dark",
        "one-light" | "onelight" => "one-light",
        "solarized" | "solarized-dark" => "solarized",
        "solarized-light" => "solarized-light",
        "kanagawa" => "kanagawa",
        "kanagawa-lotus" | "lotus" => "kanagawa-lotus",
        "rose-pine" | "rosepine" => "rose-pine",
        "rose-pine-dawn" | "rosepine-dawn" | "dawn" => "rose-pine-dawn",
        "vesper" => "vesper",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> HerdrTheme {
        resolve(&toml::from_str::<Config>(text).unwrap().theme)
    }

    #[test]
    fn named_theme_with_aliases() {
        assert_eq!(parse("[theme]\nname = \"rose-pine\"").name, "rose-pine");
        assert_eq!(parse("[theme]\nname = \"Tokyo Night\"").name, "tokyo-night");
        assert_eq!(parse("[theme]\nname = \"dawn\"").name, "rose-pine-dawn");
    }

    #[test]
    fn defaults_like_herdr() {
        assert_eq!(parse("").name, "catppuccin");
        assert_eq!(parse("[theme]\nname = \"no-such-theme\"").name, "catppuccin");
        assert_eq!(parse("[keys]\nprefix = \"ctrl+b\"").name, "catppuccin");
    }

    #[test]
    fn auto_switch_sends_both() {
        let theme = parse("[theme]\nname = \"nord\"\nauto_switch = true\nlight_name = \"solarized-light\"");
        assert_eq!(theme.dark.as_deref(), Some("catppuccin"));
        assert_eq!(theme.light.as_deref(), Some("solarized-light"));
        assert_eq!(parse("[theme]\nname = \"nord\"").dark, None);
    }
}
