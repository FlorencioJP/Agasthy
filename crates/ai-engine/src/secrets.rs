//! API keys live in the operating system's credential store (macOS Keychain,
//! Windows Credential Manager, Linux kernel keyring). They are never written
//! to AGASTHY's database, logs or files, and never passed on a command line.

const SERVICE: &str = "org.agasthy.desktop";

fn entry(preset: &str) -> Result<keyring::Entry, String> {
    if preset.is_empty()
        || !preset
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("invalid provider id".into());
    }
    keyring::Entry::new(SERVICE, &format!("provider:{preset}")).map_err(|e| e.to_string())
}

pub fn set_key(preset: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.len() > 4096 || key.contains(['\n', '\r']) {
        return Err("invalid key".into());
    }
    entry(preset)?.set_password(key).map_err(|e| e.to_string())
}

pub fn get_key(preset: &str) -> Result<Option<String>, String> {
    match entry(preset)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn has_key(preset: &str) -> bool {
    matches!(get_key(preset), Ok(Some(_)))
}

pub fn delete_key(preset: &str) -> Result<(), String> {
    match entry(preset)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
