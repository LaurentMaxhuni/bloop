//! Self-update dry run against a local fake GitHub and a real Inno Setup installer.
//!
//!     cargo run --example update_dry_run
//!
//! Builds two installers from `installer/bloop.iss` with `/DFixture` (a separate app with its
//! own AppId, mutex, folder and shortcuts; it never touches an installed bloop), installs the
//! old one, then plays a running old bloop: holds the fixture mutex, finds the newer release,
//! downloads and verifies it with the updater's own code, starts setup exactly as the app
//! does, and quits a few seconds later. Passes when setup waited for the mutex, upgraded, and
//! relaunched the app with `--background`. The fixture is uninstalled at the end.
//!
//! The installed "app" is this example's own exe renamed to `bloop.exe`: started with
//! `--background` it writes `relaunched.txt` next to itself and exits. Needs Inno Setup 6.
use bloop::update::{self, Version};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

const TITLE: &str = "bloop-update-fixture";
const MUTEX: &str = r"Local\bloop-update-fixture";
const OLD: &str = "0.0.1";
const NEW: &str = "9.9.9";
/// How long the "old bloop" keeps its mutex after starting setup (a slow quit).
const QUIT_DELAY: Duration = Duration::from_secs(4);

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|a| a == "--background") {
        // Installed fixture app, started by setup's [Run] entry.
        let exe = std::env::current_exe().unwrap();
        let note = format!("{} {}\n", unix_millis(), args[1..].join(" "));
        std::fs::write(exe.with_file_name("relaunched.txt"), note).unwrap();
        return;
    }
    match run() {
        Ok(()) => println!("PASS"),
        Err(error) => {
            eprintln!("FAIL: {error}");
            std::process::exit(1);
        }
    }
}

fn step(text: &str) {
    println!("- {text}");
}

fn run() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let work = std::env::temp_dir().join(format!("bloop-update-dry-run-{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let install_dir = PathBuf::from(std::env::var("LOCALAPPDATA").map_err(|e| e.to_string())?)
        .join("Programs")
        .join(TITLE);
    if install_dir.exists() {
        return Err(format!(
            "{} exists; uninstall the old fixture first",
            install_dir.display()
        ));
    }
    let result = scenario(&root, &work, &install_dir);
    step("uninstalling the fixture");
    uninstall(&install_dir);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn scenario(root: &Path, work: &Path, install_dir: &Path) -> Result<()> {
    let iscc = find_iscc()?;
    let payload = work.join("payload").join("bloop.exe");
    std::fs::create_dir_all(payload.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(std::env::current_exe().unwrap(), &payload).map_err(|e| e.to_string())?;
    let release_dir = work.join("release");
    for version in [OLD, NEW] {
        step(&format!("building fixture installer {version}"));
        let status = Command::new(&iscc)
            .arg("/Qp")
            .arg("/DFixture")
            .arg(format!("/DAppVersion={version}"))
            .arg(format!("/DAppExe={}", payload.display()))
            .arg(format!("/DOutDir={}", release_dir.display()))
            .arg(root.join("installer").join("bloop.iss"))
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(format!("ISCC failed: {status}"));
        }
    }
    let old_setup = release_dir.join(update::installer_name(&v(OLD)));
    let new_setup = release_dir.join(update::installer_name(&v(NEW)));

    step(&format!(
        "installing {OLD} silently (no autostart, no desktop icon)"
    ));
    let status = Command::new(&old_setup)
        .args([
            "/VERYSILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/MERGETASKS=!autostart,!desktopicon",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() || installed_version().as_deref() != Some(OLD) {
        return Err(format!("installing {OLD} failed: {status}"));
    }

    step("serving a fake GitHub release");
    let bytes = std::fs::read(&new_setup).map_err(|e| e.to_string())?;
    let base = serve(&bytes, &update::installer_name(&v(NEW)))?;

    step(&format!(
        "old bloop {OLD} starts and takes the fixture mutex"
    ));
    let mut mutex = hold_mutex(QUIT_DELAY)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let updates = work.join("updates");
    let (release, path) = runtime.block_on(async {
        let release = update::latest_from(&format!("{base}/latest"), &v(OLD))
            .await
            .map_err(|e| e.to_string())?
            .ok_or("the check found no newer release")?;
        println!("  found {} ({} bytes)", release.version, release.size);
        let path = update::download_from(&release, &updates, |_| {})
            .await
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((release, path))
    })?;
    let ready = update::ready(&updates, &v(OLD));
    if ready.as_ref() != Some(&(release.version, path.clone())) {
        return Err(format!("ready() didn't accept the download: {ready:?}"));
    }
    println!("  downloaded and verified {}", path.display());

    step("starting setup exactly as the app does, then quitting slowly");
    let started = Instant::now();
    let mut setup = update::installer_command(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    let released = mutex.wait().map_err(|e| e.to_string())?;
    let held_for = started.elapsed();
    let released_at = unix_millis();
    if !released.success() {
        return Err("the mutex holder failed".into());
    }
    println!("  old bloop quit after {held_for:.1?}");
    let status = setup.wait().map_err(|e| e.to_string())?;
    println!(
        "  setup exited with {status} after {:.1?}",
        started.elapsed()
    );
    if !status.success() {
        return Err(format!(
            "setup failed; log:\n{}",
            std::fs::read_to_string(updates.join("install.log")).unwrap_or_default()
        ));
    }
    if installed_version().as_deref() != Some(NEW) {
        return Err(format!("installed version is {:?}", installed_version()));
    }
    println!("  installed version is now {NEW}");

    let marker = install_dir.join("relaunched.txt");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let note = std::fs::read_to_string(&marker).map_err(|_| "setup never relaunched the app")?;
    if !note.trim_end().ends_with("--background") {
        return Err(format!("relaunched with the wrong arguments: {note}"));
    }
    let stamp: u128 = note.split(' ').next().unwrap().parse().unwrap();
    if stamp < released_at {
        return Err("setup relaunched the app before the old copy quit".into());
    }
    println!(
        "  setup waited: relaunched {} ms after the old copy let go of the mutex",
        stamp - released_at
    );
    println!(
        "  relaunched: bloop.exe {}",
        note.split_once(' ').unwrap().1.trim()
    );
    let log = std::fs::read_to_string(updates.join("install.log")).unwrap_or_default();
    if !log.contains("Installation process succeeded") {
        return Err("install.log doesn't record a successful install".into());
    }
    println!("  install.log written next to the installer");
    Ok(())
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn find_iscc() -> Result<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    [
        format!(r"{local}\Programs\Inno Setup 6\ISCC.exe"),
        r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe".into(),
        r"C:\Program Files\Inno Setup 6\ISCC.exe".into(),
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
    .ok_or_else(|| "Inno Setup 6 not found (winget install JRSoftware.InnoSetup)".into())
}

/// The fixture's version from its uninstall entry, read with reg.exe.
fn installed_version() -> Option<String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\{3C1F6A52-7E0B-4D4B-9B57-0F4C2A9E1D11}_is1";
    let out = Command::new("reg")
        .args(["query", key, "/v", "DisplayVersion"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.split("REG_SZ").nth(1))
        .map(|v| v.trim().to_owned())
}

/// Holds the named mutex in a child PowerShell for `delay` after it is taken, like a bloop
/// that takes a while to quit. Returns once the mutex is held.
fn hold_mutex(delay: Duration) -> Result<std::process::Child> {
    let script = format!(
        "$m = New-Object System.Threading.Mutex($true, '{MUTEX}'); [Console]::Out.WriteLine('held'); [Console]::Out.Flush(); Start-Sleep -Milliseconds {}; $m.ReleaseMutex(); $m.Dispose()",
        delay.as_millis()
    );
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut line = [0u8; 4];
    child
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut line)
        .map_err(|e| format!("mutex holder: {e}"))?;
    Ok(child)
}

/// Serves `/latest`, `/exe` and `/sha` on 127.0.0.1 from a background thread.
fn serve(installer: &[u8], name: &str) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let base = format!("http://{}", listener.local_addr().unwrap());
    let digest: [u8; 32] = Sha256::digest(installer).into();
    let checksum = format!("{}  {name}\n", update::hex(&digest));
    let latest = format!(
        r#"{{"tag_name":"v{NEW}","draft":false,"prerelease":false,"assets":[
        {{"name":"{name}","size":{},"browser_download_url":"{base}/exe"}},
        {{"name":"{name}.sha256","size":{},"browser_download_url":"{base}/sha"}}]}}"#,
        installer.len(),
        checksum.len()
    );
    let installer = installer.to_vec();
    std::thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            let mut socket = socket;
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&buf[..n]),
                }
            }
            let path = String::from_utf8_lossy(&head)
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_owned();
            let body: &[u8] = match path.as_str() {
                "/latest" => latest.as_bytes(),
                "/exe" => &installer,
                "/sha" => checksum.as_bytes(),
                _ => b"",
            };
            let status = if body.is_empty() {
                "404 Not Found"
            } else {
                "200 OK"
            };
            let _ = write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(body);
        }
    });
    Ok(base)
}

fn uninstall(install_dir: &Path) {
    let uninstaller = install_dir.join("unins000.exe");
    if uninstaller.exists() {
        let _ = Command::new(&uninstaller)
            .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .status();
        // The uninstaller copies itself to %TEMP% and returns before it finishes.
        let deadline = Instant::now() + Duration::from_secs(15);
        while installed_version().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let _ = std::fs::remove_file(install_dir.join("relaunched.txt"));
    let _ = std::fs::remove_dir(install_dir);
}
