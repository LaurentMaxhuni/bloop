//! The API key for HTTP providers, kept in Windows Credential Manager (never in SQLite or logs).

/// Where the key in use comes from, for the Settings status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Saved,
    None,
}

const TARGET: &str = "bloop/api-key";
const MAX_KEY: usize = 4096;

/// `BLOOP_API_KEY` if set, else the saved key.
pub fn api_key() -> Option<String> {
    std::env::var("BLOOP_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| read(TARGET))
}

pub fn key_source() -> KeySource {
    if std::env::var("BLOOP_API_KEY").is_ok_and(|k| !k.trim().is_empty()) {
        KeySource::Environment
    } else if read(TARGET).is_some() {
        KeySource::Saved
    } else {
        KeySource::None
    }
}

/// Saves the key, or removes it for `None` / a blank key.
pub fn set_api_key(key: Option<&str>) -> Result<(), String> {
    write(TARGET, key)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
fn read(target: &str) -> Option<String> {
    use windows::{
        Win32::Security::Credentials::{CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW},
        core::PCWSTR,
    };
    let target = wide(target);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: NUL-terminated target; on success Windows allocates `credential`, freed below.
    unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut credential) }
        .ok()?;
    // SAFETY: CredReadW succeeded, so `credential` points to a valid CREDENTIALW whose blob
    // pointer/size describe one allocation; we copy it out before freeing.
    let key = unsafe {
        let c = &*credential;
        let blob = if c.CredentialBlob.is_null() {
            &[][..]
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
        };
        let key = String::from_utf8(blob.to_vec()).ok();
        CredFree(credential.cast());
        key
    };
    key.filter(|k| !k.trim().is_empty())
}

#[cfg(windows)]
fn write(target: &str, key: Option<&str>) -> Result<(), String> {
    use windows::{
        Win32::{
            Foundation::ERROR_NOT_FOUND,
            Security::Credentials::{
                CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW,
                CredWriteW,
            },
        },
        core::{PCWSTR, PWSTR},
    };
    let mut name = wide(target);
    let key = key.map(str::trim).filter(|k| !k.is_empty());
    let Some(key) = key else {
        // SAFETY: NUL-terminated target outlives the call.
        return match unsafe { CredDeleteW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
            Err(e) => Err(format!("Could not remove the key: {}", e.message())),
        };
    };
    if key.len() > MAX_KEY {
        return Err("The key is too long (4 KiB at most).".into());
    }
    let mut blob = key.as_bytes().to_vec();
    let mut user = wide("bloop");
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(name.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: every pointer in `credential` borrows a Vec that lives until after the call.
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|e| format!("Could not save the key: {}", e.message()))
}

#[cfg(not(windows))]
fn read(_target: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
fn write(_target: &str, _key: Option<&str>) -> Result<(), String> {
    Err("Saving keys is only available on Windows; set BLOOP_API_KEY instead.".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    const TARGET: &str = "bloop-test/api-key";

    #[test]
    fn credential_round_trip() {
        write(TARGET, Some("sk-unsloth-abc")).unwrap();
        assert_eq!(read(TARGET).as_deref(), Some("sk-unsloth-abc"));
        write(TARGET, None).unwrap();
        assert_eq!(read(TARGET), None);
        write(TARGET, None).unwrap();
    }

    #[test]
    fn blank_key_removes() {
        let target = "bloop-test/blank-key";
        write(target, Some("sk-x")).unwrap();
        write(target, Some("   ")).unwrap();
        assert_eq!(read(target), None);
    }

    #[test]
    fn oversized_key_rejected() {
        assert!(write("bloop-test/big-key", Some(&"k".repeat(4097))).is_err());
    }
}
