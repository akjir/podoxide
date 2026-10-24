//! Podman CLI version verification and runtime inspection queries.

use crate::io::exec::command;

/// Maximum number of lines scanned in `podman --version` output.
const MAX_VERSION_LINES: usize = 32;
/// Maximum byte length accepted for `podman --version` output.
const MAX_VERSION_OUTPUT_BYTES: usize = 4096;
/// Minimum supported Podman major version.
const MIN_PODMAN_MAJOR: u32 = 5;
/// Minimum supported Podman minor version when major equals [`MIN_PODMAN_MAJOR`].
const MIN_PODMAN_MINOR: u32 = 8;

/// Evaluates whether the installed Podman CLI is version 5.8.0 or higher.
#[must_use]
pub fn check_podman_version() -> bool {
    let Some(output) = command::exec_capture("podman", &["--version"]) else {
        return false;
    };
    is_supported_podman_version(&output)
}

/// Evaluates whether a `podman --version` output string satisfies version 5.8.0 or higher.
#[must_use]
pub fn is_supported_podman_version(output: &str) -> bool {
    let Some((major, minor)) = parse_podman_major_minor(output) else {
        return false;
    };
    major > MIN_PODMAN_MAJOR || (major == MIN_PODMAN_MAJOR && minor >= MIN_PODMAN_MINOR)
}

fn extract_version_token(line: &str) -> Option<&str> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "podman" || tokens.next()? != "version" {
        return None;
    }
    let version_token = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some(version_token)
}

fn is_valid_patch_segment(patch_remainder: &str) -> bool {
    let digit_end = patch_remainder
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(patch_remainder.len());
    let (patch_digits, suffix) = patch_remainder.split_at(digit_end);
    if parse_ascii_u32(patch_digits).is_none() {
        return false;
    }
    is_valid_version_suffix(suffix)
}

fn is_valid_version_suffix(suffix: &str) -> bool {
    if suffix.is_empty() {
        return true;
    }
    let Some(metadata) = suffix
        .strip_prefix('-')
        .or_else(|| suffix.strip_prefix('+'))
    else {
        return false;
    };
    !metadata.is_empty()
        && metadata.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'+'
        })
}

fn parse_ascii_u32(segment: &str) -> Option<u32> {
    if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    segment.parse::<u32>().ok()
}

fn parse_line_major_minor(line: &str) -> Option<(u32, u32)> {
    let version_token = extract_version_token(line)?;
    let (major_str, remainder) = version_token.split_once('.')?;
    let (minor_str, patch_remainder) = remainder.split_once('.')?;
    if !is_valid_patch_segment(patch_remainder) {
        return None;
    }
    let major = parse_ascii_u32(major_str)?;
    let minor = parse_ascii_u32(minor_str)?;
    Some((major, minor))
}

fn parse_podman_major_minor(output: &str) -> Option<(u32, u32)> {
    if output.len() > MAX_VERSION_OUTPUT_BYTES {
        return None;
    }
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_VERSION_LINES {
            return None;
        }
        if let Some(version) = parse_line_major_minor(line) {
            return Some(version);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_VERSION_LINES, MAX_VERSION_OUTPUT_BYTES, check_podman_version,
        is_supported_podman_version,
    };

    #[test]
    fn check_podman_version_and_version_parser_edge_cases() {
        assert!(check_podman_version());

        assert!(is_supported_podman_version("podman version 5.8.0\n"));
        assert!(is_supported_podman_version("podman version 5.8.1"));
        assert!(is_supported_podman_version("podman version 5.9.0-rc1"));
        assert!(is_supported_podman_version("podman version 6.0.0"));
        assert!(is_supported_podman_version("podman version 6.1.4\n"));

        assert!(!is_supported_podman_version("podman version 5.7.9"));
        assert!(!is_supported_podman_version("podman version 4.9.3"));
        assert!(!is_supported_podman_version("podman version 5.8"));
        assert!(!is_supported_podman_version("podman version 5.8.x"));
        assert!(!is_supported_podman_version("podman version x.8.0"));
        assert!(!is_supported_podman_version("podman version 5.x.0"));
        assert!(!is_supported_podman_version("podman version"));
        assert!(!is_supported_podman_version("podman 5.8.0"));
        assert!(!is_supported_podman_version(""));
    }

    #[test]
    fn is_supported_podman_version_rejects_spoofed_and_malformed_inputs() {
        assert!(is_supported_podman_version(
            "WARN[0000] cgroup version 2 check\npodman version 5.8.0\n"
        ));
        assert!(is_supported_podman_version(
            "podman version 5.8.0+build.123"
        ));

        assert!(!is_supported_podman_version("podman version5.8.0"));
        assert!(!is_supported_podman_version("docker version 24.0.0"));
        assert!(!is_supported_podman_version("subversion 5.8.0"));
        assert!(!is_supported_podman_version("inversion 6.0.0"));
        assert!(!is_supported_podman_version("podman version 5.8.0 extra"));
        assert!(!is_supported_podman_version("podman version +5.8.0"));
        assert!(!is_supported_podman_version("podman version 5.+8.0"));
        assert!(!is_supported_podman_version("podman version +5.+8.0"));
        assert!(!is_supported_podman_version("podman version 5.8.+0"));
        assert!(!is_supported_podman_version("podman version 5.8.0.1"));
        assert!(!is_supported_podman_version("podman version 5.8.0..1"));
        assert!(!is_supported_podman_version("podman version 5.8.0bad"));
        assert!(!is_supported_podman_version("podman version 5.8.0ä"));
        assert!(!is_supported_podman_version("podman version 5.8.0-"));
        assert!(!is_supported_podman_version("podman version 5.8.0+"));
        assert!(!is_supported_podman_version("podman version 5.8.0-rc1;id"));
        assert!(!is_supported_podman_version(
            "podman version 5.8.99999999999999999999999999999"
        ));

        let too_many_lines = format!(
            "{}podman version 5.8.0\n",
            "noise\n".repeat(MAX_VERSION_LINES)
        );
        assert!(!is_supported_podman_version(&too_many_lines));

        let oversized = format!(
            "{}podman version 5.8.0",
            " ".repeat(MAX_VERSION_OUTPUT_BYTES)
        );
        assert!(!is_supported_podman_version(&oversized));
    }
}
