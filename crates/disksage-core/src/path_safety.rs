//! Path expansion and deletion-safety primitives shared by the UI process and
//! the elevated helper.
//!
//! These checks are deliberately conservative. A rule may only delete matched
//! descendants of its declared roots, and system red lines always win.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::error::{CoreError, Result};

/// Environment variables that rules may reference.
pub const ALLOWED_ENV_VARS: &[&str] = &[
    "TEMP",
    "TMP",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
    "PROGRAMDATA",
    "WINDIR",
    "SYSTEMDRIVE",
];

pub type EnvMap = HashMap<String, String>;

fn env_lookup<'a>(env: &'a EnvMap, name: &str) -> Option<&'a str> {
    env.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn without_verbatim(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

/// Expand `%VAR%` placeholders from an allowlisted environment map.
///
/// Wildcards are rejected: rule matching expands wildcards separately, while
/// this function is used for literal deletion targets.
pub fn expand_env(input: &str, env: &EnvMap) -> Result<PathBuf> {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            output.push('%');
            rest = after;
            continue;
        };
        let name = &after[..end];
        if name.is_empty() {
            output.push('%');
            rest = after;
            continue;
        }
        if !ALLOWED_ENV_VARS
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name))
        {
            return Err(CoreError::EnvNotAllowed(name.to_string()));
        }
        let value = env_lookup(env, name).ok_or_else(|| CoreError::EnvMissing(name.to_string()))?;
        output.push_str(value);
        rest = &after[end + 1..];
    }
    output.push_str(rest);

    if output.contains('*') || output.contains('?') {
        return Err(CoreError::WildcardInLiteral(output));
    }

    let path = PathBuf::from(output);
    validate_local_absolute(&path)?;
    Ok(path)
}

/// Accept only absolute local drive paths.
///
/// Rejected: relative paths, UNC paths, device namespaces, `\\.\` devices,
/// `..` components, and verbatim UNC/device prefixes. Verbatim drive paths
/// (`\\?\C:\...`) are accepted because Windows APIs may return them.
pub fn validate_local_absolute(path: &Path) -> Result<()> {
    let display = path.to_string_lossy();
    if display.is_empty() {
        return Err(CoreError::InvalidPath("empty path".into()));
    }

    let plain = without_verbatim(&display);
    let normalized_separators = plain.replace('/', "\\");

    if normalized_separators.starts_with("\\\\")
        || normalized_separators.starts_with("UNC\\")
        || normalized_separators.starts_with("\\\\.\\")
    {
        return Err(CoreError::InvalidPath(display.into_owned()));
    }

    if !path.is_absolute() {
        return Err(CoreError::InvalidPath(display.into_owned()));
    }

    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CoreError::InvalidPath(display.into_owned()));
    }

    Ok(())
}

/// Lowercase, slash-normalized form used only for comparisons. The original
/// path is retained for display and file operations.
pub fn normalize_for_compare(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let plain = without_verbatim(&raw).replace('/', "\\");
    let trimmed = plain.trim_end_matches('\\');
    let result = if trimmed.is_empty() { &plain } else { trimmed };
    result.to_ascii_lowercase()
}

/// True when `path` is `root` itself or a descendant, using Windows
/// case-insensitive comparison and a separator boundary.
pub fn is_under(path: &Path, root: &Path) -> bool {
    let candidate = normalize_for_compare(path);
    let base = normalize_for_compare(root);
    if candidate == base {
        return true;
    }
    candidate.starts_with(&format!("{base}\\"))
}

/// System locations that may never be deleted by a rule or a user.
pub fn system_red_line_roots(env: &EnvMap) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(windir) = env_lookup(env, "WINDIR") {
        let windir = PathBuf::from(windir);
        for child in [
            "WinSxS",
            "Installer",
            "WindowsApps",
            "System32",
            "SysWOW64",
            "Boot",
            "Fonts",
            "servicing",
        ] {
            roots.push(windir.join(child));
        }
    }

    if let Some(system_drive) = env_lookup(env, "SYSTEMDRIVE") {
        let drive = PathBuf::from(format!("{}\\", system_drive.trim_end_matches('\\')));
        for child in [
            "System Volume Information",
            "Recovery",
            "Boot",
            "$Recycle.Bin",
        ] {
            roots.push(drive.join(child));
        }
    }

    if let Some(program_data) = env_lookup(env, "PROGRAMDATA") {
        let program_data = PathBuf::from(program_data);
        roots.push(program_data.join("Microsoft").join("Crypto"));
        roots.push(
            program_data
                .join("Microsoft")
                .join("Windows")
                .join("SystemData"),
        );
    }

    roots
}

/// Exact files that are never deletion targets.
pub fn system_red_line_files(env: &EnvMap) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(system_drive) = env_lookup(env, "SYSTEMDRIVE") {
        let drive = PathBuf::from(format!("{}\\", system_drive.trim_end_matches('\\')));
        for name in ["pagefile.sys", "hiberfil.sys", "swapfile.sys"] {
            files.push(drive.join(name));
        }
    }
    files
}

/// True when a path is inside a protected system location or is a protected
/// system file. Descendants are protected, not just the root.
pub fn is_system_red_line(path: &Path, env: &EnvMap) -> bool {
    if system_red_line_roots(env)
        .iter()
        .any(|root| is_under(path, root))
    {
        return true;
    }
    let candidate = normalize_for_compare(path);
    system_red_line_files(env)
        .iter()
        .any(|file| normalize_for_compare(file) == candidate)
}

/// Roots that may not themselves be deletion targets. Descendants may be
/// targeted only when a rule explicitly matches them and they are not a system
/// red line.
pub fn forbidden_target_roots(env: &EnvMap) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(system_drive) = env_lookup(env, "SYSTEMDRIVE") {
        let drive = PathBuf::from(format!("{}\\", system_drive.trim_end_matches('\\')));
        roots.push(drive.clone());
        roots.push(drive.join("Program Files"));
        roots.push(drive.join("Program Files (x86)"));
    }
    for var in [
        "WINDIR",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
    ] {
        if let Some(value) = env_lookup(env, var) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(profile) = env_lookup(env, "USERPROFILE") {
        let profile = PathBuf::from(profile);
        for child in [
            "Documents",
            "Desktop",
            "Downloads",
            "Pictures",
            "Videos",
            "Music",
            "OneDrive",
        ] {
            roots.push(profile.join(child));
        }
    }

    roots
}

/// Reject a deletion target that is a forbidden root itself or a system red
/// line. Callers still need to verify that the target is a matched descendant
/// of the rule's declared roots.
pub fn is_forbidden_delete_target(path: &Path, env: &EnvMap) -> bool {
    if is_system_red_line(path, env) {
        return true;
    }
    let candidate = normalize_for_compare(path);
    forbidden_target_roots(env)
        .iter()
        .any(|root| normalize_for_compare(root) == candidate)
}

/// True when `path` is strictly below `root` (not equal).
pub fn is_strict_descendant(path: &Path, root: &Path) -> bool {
    is_under(path, root) && normalize_for_compare(path) != normalize_for_compare(root)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn env() -> EnvMap {
        [
            ("TEMP", r"C:\Users\alice\AppData\Local\Temp"),
            ("TMP", r"C:\Users\alice\AppData\Local\Temp"),
            ("LOCALAPPDATA", r"C:\Users\alice\AppData\Local"),
            ("APPDATA", r"C:\Users\alice\AppData\Roaming"),
            ("USERPROFILE", r"C:\Users\alice"),
            ("PROGRAMDATA", r"C:\ProgramData"),
            ("WINDIR", r"C:\Windows"),
            ("SYSTEMDRIVE", "C:"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
    }

    #[test]
    fn expands_allowed_environment_variables() {
        let env = env();
        let path = expand_env(r"%TEMP%\example.log", &env).unwrap();
        assert_eq!(
            path,
            PathBuf::from(r"C:\Users\alice\AppData\Local\Temp\example.log")
        );
    }

    #[test]
    fn rejects_unknown_environment_variables() {
        let env = env();
        let error = expand_env(r"%PATH%\evil.exe", &env).unwrap_err();
        assert!(matches!(error, CoreError::EnvNotAllowed(_)));
    }

    #[test]
    fn rejects_missing_environment_variables() {
        let mut env = env();
        env.remove("TEMP");
        let error = expand_env(r"%TEMP%\file", &env).unwrap_err();
        assert!(matches!(error, CoreError::EnvMissing(_)));
    }

    #[test]
    fn rejects_wildcards_for_literal_targets() {
        let env = env();
        let error = expand_env(r"%TEMP%\*", &env).unwrap_err();
        assert!(matches!(error, CoreError::WildcardInLiteral(_)));
    }

    #[test]
    fn rejects_traversal_and_relative_and_unc() {
        let env = env();
        assert!(expand_env(r"C:\Users\alice\..\Windows\System32", &env).is_err());
        assert!(validate_local_absolute(Path::new(r"relative\file")).is_err());
        assert!(validate_local_absolute(Path::new(r"\\server\share\file")).is_err());
        assert!(validate_local_absolute(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }

    #[test]
    fn comparison_is_case_insensitive_and_boundary_aware() {
        let windows = Path::new(r"C:\Windows");
        assert!(is_under(Path::new(r"c:\windows\WinSxS"), windows));
        assert!(!is_under(Path::new(r"C:\Windows10\file"), windows));
        assert_eq!(
            normalize_for_compare(Path::new(r"\\?\C:\Windows\")),
            r"c:\windows"
        );
    }

    #[test]
    fn protects_system_red_lines() {
        let env = env();
        assert!(is_system_red_line(
            Path::new(r"C:\Windows\WinSxS\file"),
            &env
        ));
        assert!(is_system_red_line(
            Path::new(r"C:\Windows\Installer\a.msi"),
            &env
        ));
        assert!(is_system_red_line(Path::new(r"C:\pagefile.sys"), &env));
        assert!(!is_system_red_line(
            Path::new(r"C:\Windows\Temp\a.tmp"),
            &env
        ));
    }

    #[test]
    fn protects_root_delete_targets_but_allows_descendants() {
        let env = env();
        assert!(is_forbidden_delete_target(Path::new("C:\\"), &env));
        assert!(is_forbidden_delete_target(Path::new(r"C:\Windows"), &env));
        assert!(is_forbidden_delete_target(
            Path::new(r"C:\Users\alice\Documents"),
            &env
        ));
        assert!(is_forbidden_delete_target(
            Path::new(r"C:\Users\alice\AppData\Local"),
            &env
        ));
        assert!(!is_forbidden_delete_target(
            Path::new(r"C:\Users\alice\AppData\Local\Temp\a.tmp"),
            &env
        ));
    }

    #[test]
    fn strict_descendant_check() {
        let root = Path::new(r"C:\Users\alice\AppData\Local\Temp");
        assert!(is_strict_descendant(
            Path::new(r"C:\Users\alice\AppData\Local\Temp\a.tmp"),
            root
        ));
        assert!(!is_strict_descendant(root, root));
    }
}
