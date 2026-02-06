use std::path::PathBuf;

use crate::error::{Error, Result};

/// Minimum required CLI version.
const MIN_CLI_VERSION: &str = "2.0.0";

/// Find the `claude` CLI binary in PATH.
pub fn find_cli() -> Result<PathBuf> {
    which::which("claude").map_err(|_| Error::CliNotFound)
}

/// Check that the CLI version meets the minimum requirement.
///
/// Runs `claude --version` and parses the semver output.
pub async fn check_cli_version(cli_path: &std::path::Path) -> Result<semver::Version> {
    let output = tokio::process::Command::new(cli_path)
        .arg("--version")
        .output()
        .await
        .map_err(|e| Error::CliConnection(format!("failed to run --version: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version_str = extract_version(&stdout)
        .ok_or_else(|| Error::CliConnection(format!("cannot parse version from: {stdout}")))?;

    let version = semver::Version::parse(version_str)
        .map_err(|e| Error::CliConnection(format!("invalid semver '{version_str}': {e}")))?;

    let min = semver::Version::parse(MIN_CLI_VERSION).unwrap();
    if version < min {
        return Err(Error::CliVersionTooOld {
            found: version.to_string(),
            required: MIN_CLI_VERSION.to_string(),
        });
    }

    Ok(version)
}

/// Extract a semver version string from CLI output.
///
/// Handles formats like "claude 2.1.0" or "2.1.0" or "claude-code v2.1.0".
fn extract_version(output: &str) -> Option<&str> {
    for word in output.split_whitespace() {
        let trimmed = word.trim_start_matches('v');
        if semver::Version::parse(trimmed).is_ok() {
            return Some(trimmed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_version_from_output() {
        assert_eq!(extract_version("claude 2.1.0"), Some("2.1.0"));
        assert_eq!(extract_version("2.1.0"), Some("2.1.0"));
        assert_eq!(extract_version("claude-code v2.1.33"), Some("2.1.33"));
        assert_eq!(extract_version("no version here"), None);
    }
}
