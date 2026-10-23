//! Bearer-token auth for phone connections.
//!
//! One random token per bridge install, created on first run at
//! `~/.config/shepsky/token` (0600). Every phone presents it as the first
//! WebSocket message. Rotate by deleting the file and restarting.

use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::RngCore;
use subtle::ConstantTimeEq;

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/shepsky"))
}

pub fn load_or_create_token() -> Result<String> {
    let dir = config_dir()?;
    let path = dir.join("token");
    migrate_legacy_token(&path)?;
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let token = existing.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    write_new_token(&path, &token)?;
    tracing::info!(path = %path.display(), "generated new pairing token");
    Ok(token)
}

/// shepsky was called collie before 0.1. Carry its token over (a copy; the
/// old file stays) so phones paired back then keep working.
fn migrate_legacy_token(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let legacy = PathBuf::from(home).join(".config/collie/token");
    let Ok(token) = std::fs::read_to_string(&legacy) else {
        return Ok(());
    };
    let token = token.trim();
    if !token.is_empty() {
        write_new_token(path, token)?;
        tracing::info!(from = %legacy.display(), "carried over the pairing token from collie");
    }
    Ok(())
}

/// Create `path` (0600, never overwriting) holding `token`.
fn write_new_token(path: &Path, token: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    use std::io::Write;
    writeln!(file, "{token}")?;
    Ok(())
}

/// Constant-time-ish comparison (both sides are short hex strings).
pub fn token_matches(expected: &str, presented: &str) -> bool {
    let a = expected.as_bytes();
    let b = presented.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.ct_eq(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_compare() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "abc12"));
        assert!(!token_matches("abc123", ""));
    }
}
