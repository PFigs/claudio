use anyhow::{bail, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use tokio::sync::watch;
use tracing::{info, warn};

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

        let device_path = match self.device_path {
            Some(p) => p,
            None => find_keyboard_device()?,
        };

        info!("PTT listener using device: {device_path}, key: {:?}", self.key);

        let key = self.key;
        let handle = tokio::task::spawn_blocking(move || {
            let mut device = match Device::open(&device_path) {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to open evdev device {device_path}: {e}");
                    return;
                }
            };

            // Use blocking event loop since evdev's async requires specific runtime setup
            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() == EventType::KEY {
                                if let InputEventKind::Key(k) = ev.kind() {
                                    if k == key {
                                        match ev.value() {
                                            1 => { let _ = tx.send(true); }   // press
                                            0 => { let _ = tx.send(false); }  // release
                                            _ => {}  // repeat, ignore
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("evdev read error: {e}, retrying in 5s");
                        std::thread::sleep(std::time::Duration::from_secs(5));
                    }
                }
            }
        });

        Ok((rx, handle))
    }
}

fn find_keyboard_device() -> Result<String> {
    for entry in std::fs::read_dir("/dev/input")? {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !name.starts_with("event") {
                continue;
            }
        }

        if let Ok(device) = Device::open(&path) {
            if let Some(keys) = device.supported_keys() {
                if keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) {
                    info!(
                        "Auto-detected keyboard: {} ({})",
                        path.display(),
                        device.name().unwrap_or("unknown")
                    );
                    return Ok(path.to_string_lossy().into_owned());
                }
            }
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
