use anyhow::{anyhow, Context, Result};
use protocol::config::{Mode, Room};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::RandomState, BTreeMap, HashSet},
    fs,
    hash::{BuildHasher, Hasher},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub model_path: String,
    #[serde(default = "default_ui_language")]
    pub ui_language: UiLanguage,
    pub autoplay_account: Account,
    #[serde(default)]
    pub autoplay: AutoplaySettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Account {
    pub username: String,
    pub password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoplaySettings {
    #[serde(default)]
    pub room_policy: RoomPolicy,
    #[serde(default = "default_manual_room")]
    pub manual_room: RoomChoice,
    #[serde(default = "default_manual_mode")]
    pub manual_mode: ModeChoice,
    #[serde(default)]
    pub action_interval_ms: ActionInterval,
    #[serde(default)]
    pub max_games: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomPolicy {
    #[default]
    AutoHighest,
    Manual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomChoice {
    Bronze,
    Silver,
    Gold,
    Jade,
    Throne,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeChoice {
    FourPlayerEast,
    FourPlayerSouth,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    Zh,
    En,
    Ja,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ActionInterval {
    pub min: u64,
    pub max: u64,
}

impl Default for AutoplaySettings {
    fn default() -> Self {
        Self {
            room_policy: RoomPolicy::default(),
            manual_room: default_manual_room(),
            manual_mode: default_manual_mode(),
            action_interval_ms: ActionInterval::default(),
            max_games: None,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model_path: "models/mortal".to_string(),
            ui_language: default_ui_language(),
            autoplay_account: Account::default(),
            autoplay: AutoplaySettings::default(),
        }
    }
}

impl Default for ActionInterval {
    fn default() -> Self {
        Self {
            min: 800,
            max: 1600,
        }
    }
}

pub fn read_settings(path: &Path) -> Result<Settings> {
    let settings = read_settings_unchecked(path)?;
    validate_settings(&settings)?;
    Ok(settings)
}

pub fn read_settings_unchecked(path: &Path) -> Result<Settings> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Settings::default()),
        Err(err) => {
            return Err(err).with_context(|| format!("read settings from {}", path.display()))
        }
    };
    let settings: Settings = serde_json::from_str(&raw)
        .with_context(|| format!("parse settings from {}", path.display()))?;
    Ok(settings)
}

pub fn write_settings(path: &Path, settings: &Settings) -> Result<()> {
    let raw = serde_json::to_string_pretty(settings)?;
    fs::write(path, format!("{raw}\n"))
        .with_context(|| format!("write settings to {}", path.display()))?;
    Ok(())
}

pub fn validate_settings(settings: &Settings) -> Result<()> {
    if settings.autoplay_account.username.trim().is_empty()
        || settings.autoplay_account.password.is_empty()
    {
        return Err(anyhow!(
            "settings autoplay_account username/password are required"
        ));
    }
    if settings.model_path.trim().is_empty() {
        return Err(anyhow!("settings model_path is required"));
    }
    if settings.autoplay.action_interval_ms.min > settings.autoplay.action_interval_ms.max {
        return Err(anyhow!("action_interval_ms min must be <= max"));
    }
    Ok(())
}

pub fn manual_target(settings: &AutoplaySettings) -> Option<(Mode, Room)> {
    match settings.room_policy {
        RoomPolicy::AutoHighest => None,
        RoomPolicy::Manual => Some((settings.manual_mode.into(), settings.manual_room.into())),
    }
}

pub fn is_placeholder_device_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("rust-cli") || lower.starts_with("rust-gui")
}

pub fn is_valid_browser_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (idx, &b) in bytes.iter().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if b != b'-' {
                    return false;
                }
            }
            14 => {
                if b != b'4' {
                    return false;
                }
            }
            19 => {
                if !matches!(b, b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

pub fn generate_browser_uuid_v4() -> Result<String> {
    let mut bytes = [0u8; 16];
    fill_random_bytes(&mut bytes)?;
    // RFC 4122 version 4 and variant 10xx
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

fn fill_random_bytes(buf: &mut [u8; 16]) -> Result<()> {
    #[cfg(unix)]
    {
        if let Ok(mut file) = fs::File::open("/dev/urandom") {
            if file.read_exact(buf).is_ok() {
                return Ok(());
            }
        }
    }
    // OS-seeded RandomState fallback (works on all platforms including Windows)
    let mut h1 = RandomState::new().build_hasher();
    h1.write_usize(buf.as_ptr() as usize);
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(h1.finish() ^ 0x9e37_79b9_7f4a_7c15);
    buf[..8].copy_from_slice(&h1.finish().to_le_bytes());
    buf[8..].copy_from_slice(&h2.finish().to_le_bytes());
    Ok(())
}

pub fn default_device_store_path(settings_path: Option<&Path>) -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return PathBuf::from(home).join(".majsoul-device-ids.json");
    }
    if let Some(parent) = settings_path.and_then(|p| p.parent()) {
        if !parent.as_os_str().is_empty() {
            return parent.join(".majsoul-device-ids.json");
        }
    }
    std::env::temp_dir().join("majsoul-device-ids.json")
}

pub fn resolve_account_device_id(
    username: &str,
    configured_device_id: Option<&str>,
    settings_path: Option<&Path>,
) -> Result<String> {
    let store_path = default_device_store_path(settings_path);
    resolve_account_device_id_in_store(username, configured_device_id, &store_path)
}

pub fn resolve_account_device_id_in_store(
    username: &str,
    configured_device_id: Option<&str>,
    store_path: &Path,
) -> Result<String> {
    let key = username.trim().to_ascii_lowercase();
    if key.is_empty() {
        return match configured_device_id {
            Some(raw) if !is_placeholder_device_id(raw) => Ok(raw.trim().to_string()),
            _ => generate_browser_uuid_v4(),
        };
    }

    let mut store = load_device_store(store_path);
    store.retain(|_, v| !is_placeholder_device_id(v));

    let used_by_others: HashSet<String> = store
        .iter()
        .filter(|(other_key, _)| *other_key != &key)
        .map(|(_, val)| val.trim().to_ascii_lowercase())
        .collect();

    if let Some(raw) = configured_device_id {
        let candidate = raw.trim();
        if !is_placeholder_device_id(candidate)
            && !used_by_others.contains(&candidate.to_ascii_lowercase())
        {
            let chosen = candidate.to_string();
            if store.get(&key) != Some(&chosen) {
                store.insert(key, chosen.clone());
                save_device_store(store_path, &store)?;
            }
            return Ok(chosen);
        }
    }

    if let Some(existing) = store.get(&key) {
        if !is_placeholder_device_id(existing)
            && !used_by_others.contains(&existing.to_ascii_lowercase())
        {
            return Ok(existing.clone());
        }
    }

    let fresh = loop {
        let candidate = generate_browser_uuid_v4()?;
        if !used_by_others.contains(&candidate.to_ascii_lowercase()) {
            break candidate;
        }
    };
    store.insert(key, fresh.clone());
    save_device_store(store_path, &store)?;
    Ok(fresh)
}

fn load_device_store(path: &Path) -> BTreeMap<String, String> {
    let Ok(raw) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str::<BTreeMap<String, String>>(&raw).unwrap_or_default()
}

fn save_device_store(path: &Path, store: &BTreeMap<String, String>) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create directory {}", parent.display()))?;
        }
    }
    let payload = format!("{}\n", serde_json::to_string_pretty(store)?);
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, payload.as_bytes())
        .with_context(|| format!("write device store {}", tmp_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp_path, path)
        .with_context(|| format!("persist device store {}", path.display()))?;
    Ok(())
}

fn default_manual_room() -> RoomChoice {
    RoomChoice::Bronze
}

fn default_manual_mode() -> ModeChoice {
    ModeChoice::FourPlayerEast
}

fn default_ui_language() -> UiLanguage {
    UiLanguage::Zh
}

impl From<RoomChoice> for Room {
    fn from(value: RoomChoice) -> Self {
        match value {
            RoomChoice::Bronze => Room::Bronze,
            RoomChoice::Silver => Room::Silver,
            RoomChoice::Gold => Room::Gold,
            RoomChoice::Jade => Room::Jade,
            RoomChoice::Throne => Room::Throne,
        }
    }
}

impl From<ModeChoice> for Mode {
    fn from(value: ModeChoice) -> Self {
        match value {
            ModeChoice::FourPlayerEast => Mode::FourPlayerEast,
            ModeChoice::FourPlayerSouth => Mode::FourPlayerSouth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_settings_path(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("majsoul-autopilot-{name}-{stamp}.json"))
    }

    #[test]
    fn missing_settings_file_loads_default_without_validation() {
        let path = temp_settings_path("missing");
        let settings = read_settings_unchecked(&path).unwrap();
        assert_eq!(settings.model_path, "models/mortal");
        assert!(settings.autoplay_account.username.is_empty());
        assert!(settings.autoplay_account.password.is_empty());
    }

    #[test]
    fn strict_settings_require_credentials_for_runtime() {
        let settings = Settings::default();
        let err = validate_settings(&settings).unwrap_err().to_string();
        assert!(err.contains("username/password"));
    }

    #[test]
    fn settings_can_save_plaintext_password() {
        let path = temp_settings_path("plaintext");
        let mut settings = Settings::default();
        settings.autoplay_account.username = "user@example.com".to_string();
        settings.autoplay_account.password = "secret-password".to_string();
        write_settings(&path, &settings).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("secret-password"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn settings_preserve_ui_language_across_save() {
        let path = temp_settings_path("language");
        fs::write(
            &path,
            r#"{
  "model_path": "models/mortal",
  "ui_language": "ja",
  "autoplay_account": {
    "username": "user@example.com",
    "password": "secret-password"
  }
}"#,
        )
        .unwrap();
        let settings = read_settings_unchecked(&path).unwrap();
        write_settings(&path, &settings).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains(r#""ui_language": "ja""#));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn each_account_gets_persistent_independent_browser_uuid_v4() {
        let store = temp_settings_path("device-store");
        let id_a1 = resolve_account_device_id_in_store("Alice@Example.com", None, &store).unwrap();
        let id_a2 = resolve_account_device_id_in_store("alice@example.com", None, &store).unwrap();
        let id_b = resolve_account_device_id_in_store("bob@example.com", None, &store).unwrap();
        // Legacy placeholder and cross-account collision must both be rejected
        let id_c = resolve_account_device_id_in_store(
            "carol@example.com",
            Some("rust-cli-device"),
            &store,
        )
        .unwrap();
        let id_d = resolve_account_device_id_in_store(
            "dave@example.com",
            Some(&id_a1),
            &store,
        )
        .unwrap();

        assert!(is_valid_browser_uuid_v4(&id_a1));
        assert_eq!(id_a1, id_a2);
        assert!(is_valid_browser_uuid_v4(&id_b));
        assert!(is_valid_browser_uuid_v4(&id_c));
        assert!(is_valid_browser_uuid_v4(&id_d));
        let unique: HashSet<_> = [id_a1, id_b, id_c, id_d].into_iter().collect();
        assert_eq!(unique.len(), 4);
        let _ = fs::remove_file(store);
    }
}
