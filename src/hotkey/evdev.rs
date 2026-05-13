use std::os::fd::AsRawFd;

use anyhow::{bail, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use tokio::sync::watch;
use tracing::{info, warn};

/// Poll timeout in milliseconds. If no events arrive within this window,
/// we check whether the underlying device node was replaced (e.g. Bluetooth
/// reconnect) and reopen if needed.
const POLL_TIMEOUT_MS: i32 = 2_000;

/// Push-to-talk listener via Linux evdev.
///
/// Monitors a keyboard device for a specific key press/release and exposes
/// the PTT state via a watch channel.
pub struct PttListener {
    key: Key,
    device_path: Option<String>,
}

impl PttListener {
    pub fn new(key_name: &str, device_path: Option<String>) -> Result<Self> {
        let key = parse_key_name(key_name)?;
        Ok(Self { key, device_path })
    }

    /// Run the PTT listener. Returns a watch receiver that is `true` when
    /// the PTT key is held down.
    pub async fn run(self) -> Result<(watch::Receiver<bool>, tokio::task::JoinHandle<()>)> {
        let (tx, rx) = watch::channel(false);

        let configured_path = self.device_path.clone();
        let initial_path = match self.device_path {
            Some(p) => p,
            None => find_keyboard_device()?,
        };

        info!("PTT listener using device: {initial_path}, key: {:?}", self.key);

        let key = self.key;
        let handle = tokio::task::spawn_blocking(move || {
            let mut device = match Device::open(&initial_path) {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to open evdev device {initial_path}: {e}");
                    return;
                }
            };

            let mut device_path = initial_path;
            loop {
                // Poll with timeout so we can detect stale (deleted) device nodes
                let mut pfd = libc::pollfd {
                    fd: device.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let poll_ret = unsafe { libc::poll(&mut pfd, 1, POLL_TIMEOUT_MS) };

                if poll_ret == 0 {
                    // Timeout -- check if device node was replaced (e.g. BT reconnect)
                    if device_node_replaced(&device_path, &device) {
                        warn!("Device node replaced, reconnecting...");
                        let _ = tx.send(false);
                        match reopen_device(&configured_path) {
                            Ok((d, path)) => {
                                info!("PTT device reconnected: {}", d.name().unwrap_or("unknown"));
                                device = d;
                                device_path = path;
                            }
                            Err(_) => {
                                std::thread::sleep(std::time::Duration::from_secs(2));
                            }
                        }
                    }
                    continue;
                }

                if poll_ret < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    warn!("poll error on evdev device: {err}");
                    let _ = tx.send(false);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    continue;
                }

                // POLLHUP/POLLERR = device gone
                if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    let _ = tx.send(false);
                    warn!("evdev device hung up, reconnecting...");
                    match reopen_device(&configured_path) {
                        Ok((d, path)) => {
                            info!("PTT device reconnected: {}", d.name().unwrap_or("unknown"));
                            device = d;
                            device_path = path;
                        }
                        Err(_) => {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                        }
                    }
                    continue;
                }

                // Data ready -- read events
                let fetch_err = match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() == EventType::KEY
                                && let InputEventKind::Key(k) = ev.kind()
                                    && k == key {
                                        match ev.value() {
                                            1 => { let _ = tx.send(true); }   // press
                                            0 => { let _ = tx.send(false); }  // release
                                            _ => {}  // repeat, ignore
                                        }
                                    }
                        }
                        continue;
                    }
                    Err(e) => e,
                };

                let _ = tx.send(false);
                warn!("evdev device lost: {fetch_err}, reconnecting...");
                match reopen_device(&configured_path) {
                    Ok((d, path)) => {
                        info!("PTT device reconnected: {}", d.name().unwrap_or("unknown"));
                        device = d;
                        device_path = path;
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        });

        Ok((rx, handle))
    }
}

/// Check if the device node on disk has a different inode than our open fd.
/// This happens when a Bluetooth device reconnects and the kernel creates a
/// new device node with the same path.
fn device_node_replaced(path: &str, device: &Device) -> bool {
    use std::os::unix::fs::MetadataExt;

    let fd_ino = {
        let fd = device.as_raw_fd();
        // fstat the open fd
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            return true; // can't stat fd, assume stale
        }
        stat.st_ino
    };

    match std::fs::metadata(path) {
        Ok(meta) => meta.ino() != fd_ino,
        Err(_) => true, // path gone, definitely stale
    }
}

/// Try to reopen the evdev device after disconnection.
/// Returns the new device and the path that was opened.
fn reopen_device(configured_path: &Option<String>) -> Result<(Device, String)> {
    match configured_path {
        Some(path) => {
            let d = Device::open(path)?;
            Ok((d, path.clone()))
        }
        None => {
            let path = find_keyboard_device()?;
            let d = Device::open(&path)?;
            Ok((d, path))
        }
    }
}

fn find_keyboard_device() -> Result<String> {
    for entry in std::fs::read_dir("/dev/input")? {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str())
            && !name.starts_with("event") {
                continue;
            }

        if let Ok(device) = Device::open(&path)
            && let Some(keys) = device.supported_keys()
                && keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) {
                    info!(
                        "Auto-detected keyboard: {} ({})",
                        path.display(),
                        device.name().unwrap_or("unknown")
                    );
                    return Ok(path.to_string_lossy().into_owned());
                }
    }
    bail!(
        "No keyboard device found in /dev/input/. \
         Make sure your user is in the 'input' group: \
         sudo usermod -aG input $USER"
    )
}

fn parse_key_name(name: &str) -> Result<Key> {
    match name {
        "KEY_RIGHTCTRL" => Ok(Key::KEY_RIGHTCTRL),
        "KEY_LEFTCTRL" => Ok(Key::KEY_LEFTCTRL),
        "KEY_RIGHTALT" => Ok(Key::KEY_RIGHTALT),
        "KEY_LEFTALT" => Ok(Key::KEY_LEFTALT),
        "KEY_RIGHTSHIFT" => Ok(Key::KEY_RIGHTSHIFT),
        "KEY_LEFTSHIFT" => Ok(Key::KEY_LEFTSHIFT),
        "KEY_CAPSLOCK" => Ok(Key::KEY_CAPSLOCK),
        "KEY_SCROLLLOCK" => Ok(Key::KEY_SCROLLLOCK),
        "KEY_PAUSE" => Ok(Key::KEY_PAUSE),
        "KEY_F13" => Ok(Key::KEY_F13),
        "KEY_F14" => Ok(Key::KEY_F14),
        "KEY_F15" => Ok(Key::KEY_F15),
        _ => bail!("Unknown key name: {name}. Supported: KEY_RIGHTCTRL, KEY_LEFTCTRL, KEY_RIGHTALT, KEY_LEFTALT, KEY_RIGHTSHIFT, KEY_LEFTSHIFT, KEY_CAPSLOCK, KEY_SCROLLLOCK, KEY_PAUSE, KEY_F13-F15"),
    }
}
