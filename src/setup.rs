use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::daemon::find_ml_service_dir;

const PIPER_VOICE: &str = "en_US-lessac-medium";
const PIPER_BASE_URL: &str =
    "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium";

/// Run all preflight checks and model downloads. Returns the config (possibly updated with
/// a detected TTS model path).
pub async fn run_setup() -> Result<Config> {
    println!();

    // [1/6] System checks
    println!("[1/6] Checking system...");
    let claude_version = check_claude_cli()?;
    println!("  claude CLI: found ({claude_version})");

    let (input_dev, output_dev) = check_audio_devices();
    println!(
        "  Audio input: {}",
        input_dev.as_deref().unwrap_or("(none found)")
    );
    println!(
        "  Audio output: {}",
        output_dev.as_deref().unwrap_or("(none found)")
    );

    check_input_group()?;
    println!("  Input group: ok");

    // [2/6] Python deps
    println!("\n[2/6] Syncing Python ML dependencies...");
    sync_python_deps()?;

    // [3/6] Models
    println!("\n[3/6] Downloading models...");
    let ml_service_dir = find_ml_service_dir()?;
    ensure_whisper_model(&ml_service_dir)?;
    ensure_silero_vad(&ml_service_dir)?;
    let piper_path = ensure_piper_model()?;

    // [4/6] Write default config if missing
    println!("\n[4/6] Checking configuration...");
    let config = write_default_config(piper_path)?;

    println!("\n  Setup complete.\n");
    Ok(config)
}

fn check_claude_cli() -> Result<String> {
    let output = std::process::Command::new("claude")
        .arg("--version")
        .output()
        .context("claude CLI not found in PATH. Install it first: https://docs.anthropic.com/en/docs/claude-code")?;

    if !output.status.success() {
        bail!("claude --version failed");
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(version)
}

fn check_audio_devices() -> (Option<String>, Option<String>) {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    let input = host
        .default_input_device()
        .and_then(|d| d.name().ok());

    let output = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    (input, output)
}

fn check_input_group() -> Result<()> {
    // Read /proc/self/status to get our groups
    let status = std::fs::read_to_string("/proc/self/status")
        .context("Failed to read /proc/self/status")?;

    let groups_line = status
        .lines()
        .find(|l| l.starts_with("Groups:"))
        .context("No Groups line in /proc/self/status")?;

    let our_gids: Vec<u32> = groups_line
        .strip_prefix("Groups:")
        .unwrap_or("")
        .split_whitespace()
        .filter_map(|g| g.parse().ok())
        .collect();

    // Find the input group's gid
    let group_content = std::fs::read_to_string("/etc/group").context("Failed to read /etc/group")?;
    let input_gid = group_content
        .lines()
        .find(|l| l.starts_with("input:"))
        .and_then(|l| l.split(':').nth(2))
        .and_then(|g| g.parse::<u32>().ok());

    match input_gid {
        Some(gid) if our_gids.contains(&gid) => Ok(()),
        Some(_) => {
            let user = std::env::var("USER").unwrap_or_else(|_| "your_user".into());
            bail!(
                "User not in 'input' group (needed for PTT hotkey).\n\
                 Run: sudo usermod -aG input {user}\n\
                 Then log out and back in."
            );
        }
        None => {
            // No input group on this system -- evdev might still work if running as root
            // or via other means. Don't block on this.
            Ok(())
        }
    }
}

fn sync_python_deps() -> Result<()> {
    let ml_dir = find_ml_service_dir()?;

    let output = std::process::Command::new("uv")
        .arg("sync")
        .current_dir(&ml_dir)
        .output()
        .context("Failed to run 'uv sync'. Is uv installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("uv sync failed:\n{stderr}");
    }

    println!("  ok");
    Ok(())
}

fn ensure_whisper_model(ml_service_dir: &PathBuf) -> Result<()> {
    print!("  Whisper base: ");

    let output = std::process::Command::new("uv")
        .args(["run", "python", "-c", "from faster_whisper import WhisperModel; WhisperModel('base'); print('ok')"])
        .current_dir(ml_service_dir)
        .output()
        .context("Failed to run whisper model check")?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout);
        if out.trim().ends_with("ok") {
            println!("ok");
            return Ok(());
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("Failed to load/download Whisper model:\n{stderr}");
}

fn ensure_silero_vad(ml_service_dir: &PathBuf) -> Result<()> {
    print!("  Silero VAD: ");

    let output = std::process::Command::new("uv")
        .args([
            "run", "python", "-c",
            "import torch; torch.hub.load('snakers4/silero-vad', 'silero_vad', trust_repo=True); print('ok')",
        ])
        .current_dir(ml_service_dir)
        .output()
        .context("Failed to run Silero VAD check")?;

    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout);
        if out.trim().ends_with("ok") {
            println!("ok");
            return Ok(());
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!("Failed to load/download Silero VAD:\n{stderr}");
}

fn ensure_piper_model() -> Result<Option<PathBuf>> {
    print!("  Piper {PIPER_VOICE}: ");

    let voice_dir = piper_voice_dir();
    let onnx_path = voice_dir.join(format!("{PIPER_VOICE}.onnx"));
    let json_path = voice_dir.join(format!("{PIPER_VOICE}.onnx.json"));

    if onnx_path.exists() && json_path.exists() {
        println!("ok (cached)");
        return Ok(Some(onnx_path));
    }

    std::fs::create_dir_all(&voice_dir)
        .context("Failed to create piper voices directory")?;

    // Download .onnx
    let onnx_url = format!("{PIPER_BASE_URL}/{PIPER_VOICE}.onnx");
    download_file(&onnx_url, &onnx_path)?;

    // Download .onnx.json
    let json_url = format!("{PIPER_BASE_URL}/{PIPER_VOICE}.onnx.json");
    download_file(&json_url, &json_path)?;

    println!("ok (downloaded)");
    Ok(Some(onnx_path))
}

fn piper_voice_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/share")
        })
        .join("piper/voices")
}

fn download_file(url: &str, dest: &PathBuf) -> Result<()> {
    let status = std::process::Command::new("curl")
        .args(["-fSL", "--progress-bar", "-o"])
        .arg(dest)
        .arg(url)
        .status()
        .context("Failed to run curl. Is curl installed?")?;

    if !status.success() {
        bail!("Download failed: {url}");
    }
    Ok(())
}

fn write_default_config(piper_model: Option<PathBuf>) -> Result<Config> {
    let config_path = Config::config_path();

    if config_path.exists() {
        println!("  Config: {} (existing)", config_path.display());
        return Config::load();
    }

    let mut config = Config::default();
    if let Some(ref model_path) = piper_model {
        config.tts.model = Some(model_path.display().to_string());
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, &toml_str)?;
    println!("  Config: {} (created)", config_path.display());

    Ok(config)
}
