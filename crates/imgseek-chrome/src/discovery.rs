use crate::error::{ChromeError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChromeProfile {
    pub name: String,
    pub path: PathBuf,
    pub is_default: bool,
}

impl ChromeProfile {
    pub fn history_path(&self) -> PathBuf {
        self.path.join("History")
    }

    pub fn cache_dir(&self) -> PathBuf {
        let modern = self.path.join("Cache").join("Cache_Data");
        if modern.exists() {
            modern
        } else {
            self.path.join("Cache")
        }
    }
}

/// Returns the standard default User Data directory for the current operating system.
pub fn default_user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let path = PathBuf::from(local_app_data)
                .join("Google")
                .join("Chrome")
                .join("User Data");
            return Some(path);
        }
        if let Ok(user_profile) = std::env::var("USERPROFILE") {
            let path = PathBuf::from(user_profile)
                .join("AppData")
                .join("Local")
                .join("Google")
                .join("Chrome")
                .join("User Data");
            return Some(path);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome");
            return Some(path);
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let chrome_path = PathBuf::from(&home).join(".config").join("google-chrome");
            if chrome_path.exists() {
                return Some(chrome_path);
            }
            let chromium_path = PathBuf::from(home).join(".config").join("chromium");
            return Some(chromium_path);
        }
    }

    None
}

/// Discovers all Chrome profiles located within the given User Data directory (or default).
pub fn find_all_profiles(user_data_dir: Option<&Path>) -> Result<Vec<ChromeProfile>> {
    let base_dir = match user_data_dir {
        Some(d) => d.to_path_buf(),
        None => default_user_data_dir().ok_or_else(|| {
            ChromeError::ProfileNotFound("could not determine default User Data directory".into())
        })?,
    };

    if !base_dir.exists() || !base_dir.is_dir() {
        return Err(ChromeError::ProfileNotFound(format!(
            "User Data directory does not exist: {}",
            base_dir.display()
        )));
    }

    let mut profiles = Vec::new();

    // Check Default profile
    let default_path = base_dir.join("Default");
    if default_path.is_dir() {
        profiles.push(ChromeProfile {
            name: "Default".to_string(),
            path: default_path,
            is_default: true,
        });
    }

    // Check numbered profiles: Profile 1, Profile 2, etc.
    let entries = std::fs::read_dir(&base_dir)?;
    let mut numbered_profiles = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                if file_name.starts_with("Profile ") {
                    numbered_profiles.push(ChromeProfile {
                        name: file_name.to_string(),
                        path,
                        is_default: false,
                    });
                }
            }
        }
    }

    // Sort numbered profiles deterministically by name
    numbered_profiles.sort_by(|a, b| a.name.cmp(&b.name));
    profiles.extend(numbered_profiles);

    Ok(profiles)
}

/// Resolves a single Chrome profile from optional `--user-data-dir` and `--profile` options.
pub fn find_profile(
    user_data_dir: Option<&Path>,
    profile_arg: Option<&str>,
) -> Result<ChromeProfile> {
    // If profile_arg is an absolute or relative path to a directory that exists, use it directly
    if let Some(arg) = profile_arg {
        let direct_path = PathBuf::from(arg);
        if direct_path.is_dir() {
            let name = direct_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Custom")
                .to_string();
            let is_default = name == "Default";
            return Ok(ChromeProfile {
                name,
                path: direct_path,
                is_default,
            });
        }
    }

    let all = find_all_profiles(user_data_dir)?;

    if all.is_empty() {
        return Err(ChromeError::ProfileNotFound(
            "no Chrome profiles found in User Data directory".into(),
        ));
    }

    if let Some(name_or_arg) = profile_arg {
        for p in &all {
            if p.name == name_or_arg {
                return Ok(p.clone());
            }
        }

        // Check if user specified base_dir/profile_name directly
        let base_dir = match user_data_dir {
            Some(d) => d.to_path_buf(),
            None => default_user_data_dir().ok_or_else(|| {
                ChromeError::ProfileNotFound(
                    "could not determine default User Data directory".into(),
                )
            })?,
        };
        let target_path = base_dir.join(name_or_arg);
        if target_path.is_dir() {
            return Ok(ChromeProfile {
                name: name_or_arg.to_string(),
                path: target_path,
                is_default: name_or_arg == "Default",
            });
        }

        return Err(ChromeError::ProfileNotFound(format!(
            "profile '{}' not found in User Data directory ({})",
            name_or_arg,
            base_dir.display()
        )));
    }

    // Default choice: prefer `Default`, else first profile
    if let Some(def) = all.iter().find(|p| p.is_default) {
        return Ok(def.clone());
    }

    Ok(all[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_in_synthetic_user_data() {
        let tmp = std::env::temp_dir().join(format!(
            "chrome_discovery_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(tmp.join("Default")).unwrap();
        std::fs::create_dir_all(tmp.join("Profile 2")).unwrap();
        std::fs::create_dir_all(tmp.join("Profile 1")).unwrap();
        std::fs::create_dir_all(tmp.join("OtherDir")).unwrap();

        let profiles = find_all_profiles(Some(&tmp)).unwrap();
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].name, "Default");
        assert!(profiles[0].is_default);
        assert_eq!(profiles[1].name, "Profile 1");
        assert!(!profiles[1].is_default);
        assert_eq!(profiles[2].name, "Profile 2");
        assert!(!profiles[2].is_default);

        // find_profile Default
        let default_prof = find_profile(Some(&tmp), None).unwrap();
        assert_eq!(default_prof.name, "Default");

        // find_profile by name
        let p2 = find_profile(Some(&tmp), Some("Profile 2")).unwrap();
        assert_eq!(p2.name, "Profile 2");
        assert_eq!(p2.path, tmp.join("Profile 2"));

        // find_profile by direct path
        let p1_path_str = tmp.join("Profile 1").to_str().unwrap().to_string();
        let p1 = find_profile(None, Some(&p1_path_str)).unwrap();
        assert_eq!(p1.name, "Profile 1");

        // Non-existent profile fails gracefully
        assert!(find_profile(Some(&tmp), Some("NonExistent")).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
