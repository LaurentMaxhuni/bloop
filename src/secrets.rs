//! Saved provider keys and the ChatGPT sign-in: Windows Credential Manager and DPAPI, or the
//! Secret Service (GNOME Keyring, KWallet, KeePassXC) on Linux. Never plain text.

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
    } else if target(endpoint).is_some_and(|t| saved(&t)) {
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

/// An older bloop saved one key for every endpoint. Which server it belongs to is unknown
/// (the endpoint may have changed since), so it is never sent until the user ties it to one.
pub fn has_legacy_key() -> bool {
    saved(LEGACY_TARGET)
}

/// Ties the older bloop's key to `endpoint`'s origin, at the user's request, and removes it
/// from its old place.
pub fn bind_legacy_key(endpoint: &str) -> Result<(), String> {
    let legacy = read(LEGACY_TARGET).ok_or("There's no older key to use.")?;
    set_api_key(endpoint, Some(&legacy))?;
    write(LEGACY_TARGET, None)
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

/// Shown when Linux has no usable keyring (none running, or the unlock prompt was dismissed).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub(crate) const NO_KEYRING: &str = "bloop needs a keyring to save sign-ins (GNOME Keyring, KWallet or KeePassXC). Install or unlock one and try again.";

/// Where the ChatGPT session lives in a keyring.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
const CHATGPT_TARGET: &str = "bloop/chatgpt";

/// A secret store addressed by target name.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
trait Vault {
    fn get(&self, target: &str) -> Result<Option<Vec<u8>>, String>;
    /// `None` removes the secret (a missing one is fine).
    fn set(&self, target: &str, value: Option<&[u8]>) -> Result<(), String>;
    /// Whether a secret is saved, without reading it (so without asking to unlock).
    fn contains(&self, target: &str) -> Result<bool, String> {
        self.get(target).map(|value| value.is_some())
    }
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn read_with(vault: &dyn Vault, target: &str) -> Option<String> {
    vault
        .get(target)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|key| !key.trim().is_empty())
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn write_with(vault: &dyn Vault, target: &str, key: Option<&str>) -> Result<(), String> {
    match key.map(str::trim).filter(|k| !k.is_empty()) {
        None => vault.set(target, None),
        Some(key) if key.len() > MAX_KEY => Err("The key is too long (4 KiB at most).".into()),
        Some(key) => vault.set(target, Some(key.as_bytes())),
    }
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn save_session_with(vault: &dyn Vault, data: &[u8]) -> Result<(), String> {
    vault.set(CHATGPT_TARGET, Some(data))
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn load_session_with(vault: &dyn Vault) -> Result<Option<Vec<u8>>, String> {
    vault.get(CHATGPT_TARGET)
}

/// Saves the ChatGPT session: DPAPI-protected in bloop's database on Windows, a keyring item
/// on Linux (never in the database there).
pub(crate) fn save_chatgpt_session(data: &mut [u8]) -> Result<(), String> {
    #[cfg(windows)]
    {
        let protected = chatgpt_blob(data, true)?;
        crate::storage::save_chatgpt(&protected)
    }
    #[cfg(target_os = "linux")]
    {
        save_session_with(&linux::Keyring, data)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = data;
        Err("ChatGPT sign-in isn't available on this platform.".into())
    }
}

/// Loads the ChatGPT session saved by [`save_chatgpt_session`].
pub(crate) fn load_chatgpt_session() -> Result<Option<Vec<u8>>, String> {
    #[cfg(windows)]
    {
        let Some(mut protected) = crate::storage::load_chatgpt()? else {
            return Ok(None);
        };
        chatgpt_blob(&mut protected, false).map(Some)
    }
    #[cfg(target_os = "linux")]
    {
        load_session_with(&linux::Keyring)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Ok(None)
    }
}

/// The Secret Service through `oo7`, one short-lived connection per call on its own thread,
/// so it works from any context (UI thread, async task or blocking worker).
#[cfg(target_os = "linux")]
mod linux {
    use super::{NO_KEYRING, Vault};

    pub struct Keyring;

    fn keyring_error(error: impl std::fmt::Display) -> String {
        tracing::warn!("keyring unavailable: {error}");
        NO_KEYRING.into()
    }

    fn blocking<T: Send + 'static>(
        work: impl Future<Output = Result<T, String>> + Send + 'static,
    ) -> Result<T, String> {
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?
                .block_on(work)
        })
        .join()
        .unwrap_or_else(|_| Err(NO_KEYRING.into()))
    }

    fn attributes(target: &str) -> [(&'static str, &str); 2] {
        [("application", "bloop"), ("target", target)]
    }

    /// bloop's items for `target` in the default collection, found without unlocking it and
    /// without creating it: a desktop whose keyring was never set up shows no prompt here.
    async fn saved_items(target: &str) -> Result<Vec<oo7::dbus::Item>, String> {
        let service = oo7::dbus::Service::new().await.map_err(keyring_error)?;
        let Some(collection) = service.with_alias("default").await.map_err(keyring_error)? else {
            return Ok(Vec::new());
        };
        collection
            .search_items(&attributes(target))
            .await
            .map_err(keyring_error)
    }

    /// The default collection, created and unlocked if needed: only for saving, which the
    /// user asked for, so a keyring prompt is expected there.
    async fn open() -> Result<oo7::Keyring, String> {
        let keyring = oo7::Keyring::new().await.map_err(keyring_error)?;
        keyring.unlock().await.map_err(keyring_error)?;
        Ok(keyring)
    }

    impl Vault for Keyring {
        fn get(&self, target: &str) -> Result<Option<Vec<u8>>, String> {
            let target = target.to_owned();
            blocking(async move {
                if ashpd::is_sandboxed() {
                    let keyring = open().await?;
                    let items = keyring
                        .search_items(&attributes(&target))
                        .await
                        .map_err(keyring_error)?;
                    let Some(item) = items.first() else {
                        return Ok(None);
                    };
                    let secret = item.secret().await.map_err(keyring_error)?;
                    return Ok(Some(secret.as_bytes().to_vec()));
                }
                // Only an existing, locked item asks to unlock; nothing is created.
                let Some(item) = saved_items(&target).await?.into_iter().next() else {
                    return Ok(None);
                };
                if item.is_locked().await.map_err(keyring_error)? {
                    item.unlock(None).await.map_err(keyring_error)?;
                }
                let secret = item.secret().await.map_err(keyring_error)?;
                Ok(Some(secret.as_bytes().to_vec()))
            })
        }

        fn contains(&self, target: &str) -> Result<bool, String> {
            if ashpd::is_sandboxed() {
                return self.get(target).map(|value| value.is_some());
            }
            let target = target.to_owned();
            blocking(async move { Ok(!saved_items(&target).await?.is_empty()) })
        }

        fn set(&self, target: &str, value: Option<&[u8]>) -> Result<(), String> {
            let target = target.to_owned();
            let value = value.map(<[u8]>::to_vec);
            blocking(async move {
                let keyring = open().await?;
                let attributes = attributes(&target);
                match value {
                    None => keyring.delete(&attributes).await.map_err(keyring_error),
                    Some(bytes) => keyring
                        .create_item(&format!("bloop: {target}"), &attributes, bytes, true)
                        .await
                        .map_err(keyring_error),
                }
            })
        }
    }
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

#[cfg(target_os = "linux")]
fn read(target: &str) -> Option<String> {
    read_with(&linux::Keyring, target)
}

#[cfg(target_os = "linux")]
fn write(target: &str, key: Option<&str>) -> Result<(), String> {
    write_with(&linux::Keyring, target, key)
}

/// Whether a key is saved under `target`, for status lines. Linux checks without unlocking
/// the keyring, so opening Settings never shows a keyring prompt.
#[cfg(target_os = "linux")]
fn saved(target: &str) -> bool {
    saved_with(&linux::Keyring, target)
}

#[cfg(not(target_os = "linux"))]
fn saved(target: &str) -> bool {
    read(target).is_some()
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn saved_with(vault: &dyn Vault, target: &str) -> bool {
    vault.contains(target).unwrap_or(false)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read(_target: &str) -> Option<String> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
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

#[cfg(test)]
mod vault_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeVault(Mutex<HashMap<String, Vec<u8>>>);

    impl Vault for FakeVault {
        fn get(&self, target: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.0.lock().unwrap().get(target).cloned())
        }

        fn set(&self, target: &str, value: Option<&[u8]>) -> Result<(), String> {
            let mut map = self.0.lock().unwrap();
            match value {
                Some(bytes) => map.insert(target.into(), bytes.to_vec()),
                None => map.remove(target),
            };
            Ok(())
        }
    }

    struct NoKeyring;

    impl Vault for NoKeyring {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
            Err(NO_KEYRING.into())
        }

        fn set(&self, _: &str, _: Option<&[u8]>) -> Result<(), String> {
            Err(NO_KEYRING.into())
        }
    }

    #[test]
    fn vault_round_trip() {
        let vault = FakeVault::default();
        write_with(&vault, "t", Some("  sk-abc  ")).unwrap();
        assert_eq!(read_with(&vault, "t").as_deref(), Some("sk-abc"));
        write_with(&vault, "t", Some("   ")).unwrap();
        assert_eq!(read_with(&vault, "t"), None, "blank removes");
        assert!(write_with(&vault, "t", Some(&"k".repeat(4097))).is_err());
    }

    #[test]
    fn chatgpt_session_goes_to_the_vault() {
        let vault = FakeVault::default();
        let session = vec![b'x'; 5000];
        save_session_with(&vault, &session).unwrap();
        assert_eq!(load_session_with(&vault).unwrap(), Some(session));
        assert_eq!(
            vault.0.lock().unwrap().keys().cloned().collect::<Vec<_>>(),
            [CHATGPT_TARGET]
        );
    }

    /// A vault whose secrets are behind a lock: reading one would prompt.
    struct Locked(FakeVault);

    impl Vault for Locked {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, String> {
            panic!("a status check must not read (and so unlock) the secret");
        }

        fn set(&self, target: &str, value: Option<&[u8]>) -> Result<(), String> {
            self.0.set(target, value)
        }

        fn contains(&self, target: &str) -> Result<bool, String> {
            self.0.get(target).map(|value| value.is_some())
        }
    }

    #[test]
    fn status_checks_never_read_the_secret() {
        let vault = Locked(FakeVault::default());
        assert!(!saved_with(&vault, "t"));
        write_with(&vault, "t", Some("sk")).unwrap();
        assert!(saved_with(&vault, "t"));
        assert!(
            !saved_with(&NoKeyring, "t"),
            "no keyring counts as nothing saved"
        );
    }

    #[test]
    fn keyring_error_copy() {
        assert_eq!(
            write_with(&NoKeyring, "t", Some("sk")).unwrap_err(),
            NO_KEYRING
        );
        assert_eq!(save_session_with(&NoKeyring, b"x").unwrap_err(), NO_KEYRING);
        assert_eq!(read_with(&NoKeyring, "t"), None);
        assert!(NO_KEYRING.contains("keyring"));
    }
}
