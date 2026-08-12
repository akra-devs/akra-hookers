use std::{collections::BTreeSet, path::Path};

use serde::Serialize;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item, Table, TableLike, value};

use super::{
    CAPTURE_HOOK_TIMEOUT_SECONDS, CodexHookCommand, CodexLifecycleError, HookEvent, HookLocation,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum HookPlatform {
    Posix,
    Windows,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TrustSource {
    hook_path: String,
    platform: HookPlatform,
}

pub(super) fn prepare_config(
    config_path: &Path,
    manifest_path: &Path,
    original: Option<&[u8]>,
    previous_locations: &[HookLocation],
    installed: Option<(&[HookLocation], &CodexHookCommand)>,
) -> Result<Option<Vec<u8>>, CodexLifecycleError> {
    if original.is_none() && installed.is_none() {
        return Ok(None);
    }

    let mut document = match original {
        Some(contents) => {
            let contents = std::str::from_utf8(contents).map_err(|source| {
                CodexLifecycleError::InvalidConfigEncoding {
                    path: config_path.to_path_buf(),
                    source,
                }
            })?;
            contents.parse::<DocumentMut>().map_err(|source| {
                CodexLifecycleError::InvalidConfigToml {
                    path: config_path.to_path_buf(),
                    source,
                }
            })?
        }
        None => DocumentMut::new(),
    };
    let sources = trust_sources(manifest_path);

    if !previous_locations.is_empty()
        && let Some(state) = existing_state_table(&mut document, config_path)?
    {
        for key in state_keys(&sources, previous_locations) {
            state.remove(&key);
        }
    }

    if let Some((locations, command)) = installed {
        let state = ensure_state_table(&mut document, config_path)?;
        for source in &sources {
            for hook_path in source_path_aliases(source) {
                for location in locations {
                    let hash = trusted_hash_for_event(
                        location.event,
                        command.command_for(source.platform),
                    );
                    let key = hook_state_key(&hook_path, *location);
                    let entry = state
                        .entry(&key)
                        .or_insert_with(|| Item::Table(Table::new()));
                    let entry = entry.as_table_like_mut().ok_or_else(|| {
                        CodexLifecycleError::InvalidConfigShape(config_path.to_path_buf())
                    })?;
                    entry.insert("enabled", value(true));
                    entry.insert("trusted_hash", value(hash.clone()));
                }
            }
        }
    }

    let intended = document.to_string().into_bytes();
    if original == Some(intended.as_slice()) {
        Ok(original.map(ToOwned::to_owned))
    } else {
        Ok(Some(intended))
    }
}

fn existing_state_table<'a>(
    document: &'a mut DocumentMut,
    config_path: &Path,
) -> Result<Option<&'a mut dyn TableLike>, CodexLifecycleError> {
    let Some(hooks) = document.as_table_mut().get_mut("hooks") else {
        return Ok(None);
    };
    let hooks = hooks
        .as_table_like_mut()
        .ok_or_else(|| CodexLifecycleError::InvalidConfigShape(config_path.to_path_buf()))?;
    let Some(state) = hooks.get_mut("state") else {
        return Ok(None);
    };
    state
        .as_table_like_mut()
        .map(Some)
        .ok_or_else(|| CodexLifecycleError::InvalidConfigShape(config_path.to_path_buf()))
}

fn ensure_state_table<'a>(
    document: &'a mut DocumentMut,
    config_path: &Path,
) -> Result<&'a mut dyn TableLike, CodexLifecycleError> {
    let hooks = document
        .as_table_mut()
        .entry("hooks")
        .or_insert_with(|| Item::Table(Table::new()));
    let hooks = hooks
        .as_table_like_mut()
        .ok_or_else(|| CodexLifecycleError::InvalidConfigShape(config_path.to_path_buf()))?;
    let state = hooks
        .entry("state")
        .or_insert_with(|| Item::Table(Table::new()));
    state
        .as_table_like_mut()
        .ok_or_else(|| CodexLifecycleError::InvalidConfigShape(config_path.to_path_buf()))
}

fn state_keys(sources: &[TrustSource], locations: &[HookLocation]) -> BTreeSet<String> {
    sources
        .iter()
        .flat_map(source_path_aliases)
        .flat_map(|path| {
            locations
                .iter()
                .copied()
                .map(move |location| hook_state_key(&path, location))
        })
        .collect()
}

fn hook_state_key(hook_path: &str, location: HookLocation) -> String {
    format!(
        "{hook_path}:{}:{}:{}",
        location.event.trust_label(),
        location.group,
        location.handler
    )
}

fn trust_sources(manifest_path: &Path) -> Vec<TrustSource> {
    #[cfg(windows)]
    {
        if let Some(hook_path) = wsl_unc_to_posix(manifest_path) {
            return vec![TrustSource {
                hook_path,
                platform: HookPlatform::Posix,
            }];
        }

        let mut sources = BTreeSet::from([TrustSource {
            hook_path: preferred_windows_path(manifest_path),
            platform: HookPlatform::Windows,
        }]);
        if let Some(hook_path) = windows_path_to_wsl_mount(manifest_path) {
            sources.insert(TrustSource {
                hook_path,
                platform: HookPlatform::Posix,
            });
        }
        sources.into_iter().collect()
    }

    #[cfg(not(windows))]
    {
        vec![TrustSource {
            hook_path: manifest_path.to_string_lossy().into_owned(),
            platform: HookPlatform::Posix,
        }]
    }
}

fn source_path_aliases(source: &TrustSource) -> BTreeSet<String> {
    match source.platform {
        HookPlatform::Posix => BTreeSet::from([source.hook_path.replace('\\', "/")]),
        HookPlatform::Windows => windows_source_path_aliases(&source.hook_path),
    }
}

#[cfg(windows)]
fn preferred_windows_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('/', "\\");
    if let Some(path) = normalized.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else {
        normalized
            .strip_prefix(r"\\?\")
            .unwrap_or(&normalized)
            .to_owned()
    }
}

#[cfg(windows)]
fn windows_source_path_aliases(path: &str) -> BTreeSet<String> {
    let native = preferred_windows_path(Path::new(path));
    let mut aliases = BTreeSet::from([native.clone(), native.replace('\\', "/")]);
    let bytes = native.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        let extended = format!(r"\\?\{native}");
        aliases.insert(extended.clone());
        aliases.insert(extended.replace('\\', "/"));
    }
    aliases
}

#[cfg(not(windows))]
fn windows_source_path_aliases(path: &str) -> BTreeSet<String> {
    BTreeSet::from([path.replace('/', "\\"), path.replace('\\', "/")])
}

#[cfg(windows)]
fn windows_path_to_wsl_mount(path: &Path) -> Option<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    let bytes = normalized.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'/' {
        return None;
    }
    let drive = char::from(bytes[0]).to_ascii_lowercase();
    drive
        .is_ascii_alphabetic()
        .then(|| format!("/mnt/{drive}/{}", normalized[3..].trim_start_matches('/')))
}

#[cfg(windows)]
fn wsl_unc_to_posix(path: &Path) -> Option<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let lowercase = normalized.to_ascii_lowercase();
    let prefix_len = ["//wsl.localhost/", "//wsl$/", "//?/unc/wsl.localhost/"]
        .into_iter()
        .find_map(|prefix| lowercase.starts_with(prefix).then_some(prefix.len()))?;
    let remainder = &normalized[prefix_len..];
    let (_, linux_path) = remainder.split_once('/')?;
    Some(format!("/{}", linux_path.trim_start_matches('/')))
}

impl CodexHookCommand {
    fn command_for(&self, platform: HookPlatform) -> &str {
        match platform {
            HookPlatform::Posix => &self.command,
            HookPlatform::Windows => self.command_windows.as_deref().unwrap_or(&self.command),
        }
    }
}

#[derive(Serialize)]
struct NormalizedHookIdentity<'a> {
    event_name: &'static str,
    hooks: [NormalizedCommandHook<'a>; 1],
}

#[derive(Serialize)]
struct NormalizedCommandHook<'a> {
    #[serde(rename = "async")]
    asynchronous: bool,
    command: &'a str,
    timeout: u64,
    #[serde(rename = "type")]
    hook_type: &'static str,
}

#[cfg(test)]
fn trusted_hash(command: &str) -> String {
    trusted_hash_for_event(HookEvent::UserPromptSubmit, command)
}

fn trusted_hash_for_event(event: HookEvent, command: &str) -> String {
    let identity = NormalizedHookIdentity {
        event_name: event.trust_label(),
        hooks: [NormalizedCommandHook {
            asynchronous: false,
            command,
            timeout: CAPTURE_HOOK_TIMEOUT_SECONDS,
            hook_type: "command",
        }],
    };
    let value = serde_json::to_value(identity).expect("hook trust identity must serialize");
    let serialized = serde_json::to_vec(&canonical_json(value))
        .expect("canonical hook trust identity must serialize");
    format!("sha256:{:x}", Sha256::digest(serialized))
}

fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonical_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn user_location(group: usize, handler: usize) -> HookLocation {
        HookLocation::new(HookEvent::UserPromptSubmit, group, handler)
    }

    #[test]
    fn hash_matches_codex_0_147_normalized_hook_algorithm() {
        assert_eq!(
            trusted_hash("akra-hookers capture"),
            "sha256:bd757851234b867d380008403ac0e54873ab8ff31dc399fae293dd2d5362a26b"
        );
    }

    #[test]
    fn config_update_preserves_unrelated_settings_and_trust_entries() {
        let config_path = Path::new("/tmp/.codex/config.toml");
        let manifest_path = Path::new("/tmp/.codex/hooks.json");
        let original = br#"model = "gpt-test"

[hooks.state.'other.json:user_prompt_submit:0:0']
enabled = false
trusted_hash = "sha256:other"
"#;
        let command = CodexHookCommand::posix("akra-hookers capture");

        let updated = prepare_config(
            config_path,
            manifest_path,
            Some(original),
            &[],
            Some((&[user_location(1, 0)], &command)),
        )
        .expect("config update")
        .expect("config bytes");
        let updated = String::from_utf8(updated).expect("UTF-8 config");

        assert!(updated.contains("model = \"gpt-test\""));
        assert!(updated.contains("sha256:other"));
        assert!(updated.contains("/tmp/.codex/hooks.json:user_prompt_submit:1:0"));
        assert!(
            updated.contains(
                "sha256:bd757851234b867d380008403ac0e54873ab8ff31dc399fae293dd2d5362a26b"
            )
        );
    }

    #[test]
    fn disabling_removes_only_matching_akra_state() {
        let config_path = Path::new("/tmp/.codex/config.toml");
        let manifest_path = Path::new("/tmp/.codex/hooks.json");
        let original = br#"[hooks.state.'/tmp/.codex/hooks.json:user_prompt_submit:1:0']
enabled = true
trusted_hash = "sha256:akra"

[hooks.state.'other.json:user_prompt_submit:1:0']
enabled = true
trusted_hash = "sha256:other"
"#;

        let updated = prepare_config(
            config_path,
            manifest_path,
            Some(original),
            &[user_location(1, 0)],
            None,
        )
        .expect("config update")
        .expect("config bytes");
        let updated = String::from_utf8(updated).expect("UTF-8 config");

        assert!(!updated.contains("sha256:akra"));
        assert!(updated.contains("sha256:other"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_home_trusts_native_and_shared_wsl_paths() {
        let sources = trust_sources(Path::new(r"C:\Users\alex\.codex\hooks.json"));

        assert!(sources.contains(&TrustSource {
            hook_path: r"C:\Users\alex\.codex\hooks.json".to_owned(),
            platform: HookPlatform::Windows,
        }));
        assert!(sources.contains(&TrustSource {
            hook_path: "/mnt/c/Users/alex/.codex/hooks.json".to_owned(),
            platform: HookPlatform::Posix,
        }));
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_home_trusts_codex_native_and_canonical_aliases() {
        let sources = trust_sources(Path::new(r"\\?\C:\Users\alex\.codex\hooks.json"));
        let windows = sources
            .iter()
            .find(|source| source.platform == HookPlatform::Windows)
            .expect("Windows source");
        let aliases = source_path_aliases(windows);

        assert_eq!(windows.hook_path, r"C:\Users\alex\.codex\hooks.json");
        assert!(aliases.contains(r"C:\Users\alex\.codex\hooks.json"));
        assert!(aliases.contains("C:/Users/alex/.codex/hooks.json"));
        assert!(aliases.contains(r"\\?\C:\Users\alex\.codex\hooks.json"));
        assert!(aliases.contains("//?/C:/Users/alex/.codex/hooks.json"));
        assert!(sources.contains(&TrustSource {
            hook_path: "/mnt/c/Users/alex/.codex/hooks.json".to_owned(),
            platform: HookPlatform::Posix,
        }));
    }

    #[cfg(windows)]
    #[test]
    fn extended_manifest_path_replaces_the_native_codex_state_hash() {
        let config_path = Path::new(r"\\?\C:\Users\alex\.codex\config.toml");
        let manifest_path = Path::new(r"\\?\C:\Users\alex\.codex\hooks.json");
        let original = br#"[hooks.state.'C:\Users\alex\.codex\hooks.json:user_prompt_submit:0:0']
enabled = true
trusted_hash = "sha256:stale"
"#;
        let command = CodexHookCommand::same("akra-hookers capture");

        let updated = prepare_config(
            config_path,
            manifest_path,
            Some(original),
            &[user_location(0, 0)],
            Some((&[user_location(0, 0)], &command)),
        )
        .expect("config update")
        .expect("config bytes");
        let updated = String::from_utf8(updated).expect("UTF-8 config");
        let document = updated.parse::<DocumentMut>().expect("updated TOML");
        let key = r"C:\Users\alex\.codex\hooks.json:user_prompt_submit:0:0";

        assert!(!updated.contains("sha256:stale"));
        assert_eq!(
            document["hooks"]["state"][key]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(
            document["hooks"]["state"][key]["trusted_hash"].as_str(),
            Some("sha256:bd757851234b867d380008403ac0e54873ab8ff31dc399fae293dd2d5362a26b")
        );
    }

    #[cfg(windows)]
    #[test]
    fn shared_home_records_distinct_windows_and_posix_hashes() {
        let config_path = Path::new(r"C:\shared\.codex\config.toml");
        let manifest_path = Path::new(r"C:\shared\.codex\hooks.json");
        let command = CodexHookCommand::with_windows("echo posix", "echo windows");

        let updated = prepare_config(
            config_path,
            manifest_path,
            None,
            &[],
            Some((&[user_location(0, 0)], &command)),
        )
        .expect("config update")
        .expect("config bytes");
        let updated = String::from_utf8(updated).expect("UTF-8 config");

        assert!(updated.contains(r"C:\shared\.codex\hooks.json:user_prompt_submit:0:0"));
        assert!(updated.contains("/mnt/c/shared/.codex/hooks.json:user_prompt_submit:0:0"));
        assert!(
            updated.contains(
                "sha256:55c1a5ff19716528679009f5d9c76bb369aee7486ad00c0dd13b7f5d226a00d5"
            )
        );
        assert!(
            updated.contains(
                "sha256:defd73615a929c0312e9d3926e8f765e1582b47a00ae2c7bb24cf9dfc619ca84"
            )
        );
    }

    #[cfg(windows)]
    #[test]
    fn wsl_unc_home_uses_native_linux_hook_key() {
        let sources = trust_sources(Path::new(
            r"\\wsl.localhost\Ubuntu\home\akra\.codex\hooks.json",
        ));

        assert_eq!(
            sources,
            vec![TrustSource {
                hook_path: "/home/akra/.codex/hooks.json".to_owned(),
                platform: HookPlatform::Posix,
            }]
        );
    }
}
