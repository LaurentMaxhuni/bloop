//! Windows protection for provider credentials.

/// Where the key in use comes from, for the Settings status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Environment,
    Saved,
    None,
}

/// Where keys were saved before they were tied to an endpoint.
const LEGACY_TARGET: &str = "bloop/api-key";
const MAX_KEY: usize = 4096;

/// Saved keys belong to one endpoint origin (scheme, host and port), so a model profile
/// pointing at another server never receives a key saved for this one.
fn target(endpoint: &str) -> Option<String> {
    let url = crate::config::validate_endpoint(endpoint).ok()?;
    Some(format!(
        "bloop/api-key/{}",
        url.origin().ascii_serialization()
    ))
}

fn environment_key() -> Option<String> {
    std::env::var("BLOOP_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
}

/// `BLOOP_API_KEY` if set, else the key saved for `endpoint`'s origin.
pub fn api_key(endpoint: &str) -> Option<String> {
    environment_key().or_else(|| target(endpoint).and_then(|t| read(&t)))
}

pub fn key_source(endpoint: &str) -> KeySource {
    if environment_key().is_some() {
        KeySource::Environment
    } else if target(endpoint).and_then(|t| read(&t)).is_some() {
        KeySource::Saved
    } else {
        KeySource::None
    }
}

/// Saves the key for `endpoint`'s origin, or removes it for `None` / a blank key.
pub fn set_api_key(endpoint: &str, key: Option<&str>) -> Result<(), String> {
    let target = target(endpoint).ok_or("Enter a valid endpoint before saving a key.")?;
    write(&target, key)
}

/// Moves a key saved by an older bloop (one key for every endpoint) to `endpoint`, the
/// endpoint it was used with. Does nothing once moved or when that origin has its own key.
pub fn migrate_legacy_key(endpoint: &str) {
    let Some(legacy) = read(LEGACY_TARGET) else {
        return;
    };
    let Some(target) = target(endpoint) else {
        return;
    };
    if read(&target).is_none() && write(&target, Some(&legacy)).is_err() {
        return;
    }
    let _ = write(LEGACY_TARGET, None);
}

/// DPAPI handles OAuth sessions larger than a Credential Manager entry and binds them to
/// the current Windows user. Only the encrypted blob is written to the database.
#[cfg(windows)]
pub(crate) fn chatgpt_blob(bytes: &mut [u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
            },
        },
        core::PCWSTR,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(|_| "ChatGPT account data is too large.")?,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the input buffer and output struct are live for the call. On success DPAPI
    // allocates the output using LocalAlloc, which is released below.
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    }
    .map_err(|_| "Windows could not protect or read the saved ChatGPT account.")?;
    // SAFETY: on success output describes a valid DPAPI allocation owned by this function.
    unsafe {
        let data = if output.cbData == 0 {
            Vec::new()
        } else {
            let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
            let data = bytes.to_vec();
            if !encrypt {
                bytes.fill(0);
            }
            data
        };
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(data)
    }
}

#[cfg(not(windows))]
pub(crate) fn chatgpt_blob(_bytes: &mut [u8], _encrypt: bool) -> Result<Vec<u8>, String> {
    Err("ChatGPT sign-in is available on Windows.".into())
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
    unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    }
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
                CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredWriteW,
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
    fn keys_are_per_origin() {
        assert_eq!(
            target("https://api.example.com/v1").as_deref(),
            Some("bloop/api-key/https://api.example.com")
        );
        assert_eq!(
            target("https://api.example.com/other"),
            target("https://api.example.com/v1"),
            "same server, same key"
        );
        assert_ne!(
            target("https://api.example.com/v1"),
            target("https://evil.example.net/v1")
        );
        assert_ne!(
            target("http://127.0.0.1:8080/v1"),
            target("http://127.0.0.1:8888/v1")
        );
        assert_eq!(target("not a url"), None);
        assert!(set_api_key("not a url", Some("sk")).is_err());
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

    #[test]
    fn chatgpt_blob_protects_a_full_session() {
        let mut original = vec![b'x'; 5000];
        let mut protected = chatgpt_blob(&mut original, true).unwrap();
        assert_ne!(original, protected);
        assert_eq!(chatgpt_blob(&mut protected, false).unwrap(), original);
    }
}
