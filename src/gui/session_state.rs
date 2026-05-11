use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gpui;
use gpui::AppContext;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tracing::{debug, info, trace, warn};

use std::path::PathBuf;

use crate::ipc::protocol::{Request, SessionInfo};
use crate::gui::ipc_bridge;

/// A cloneable Write handle backed by a shared writer.
struct SharedWriter {
    inner: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.lock().unwrap().flush()
    }
}

/// Keywords in terminal output that trigger a "needs help" notification.
/// Matched case-insensitively against ANSI-stripped PTY output.
const NOTIFY_KEYWORDS: &[&str] = &[
    "do you want to proceed",
    "do you want to allow",
    "requires approval",
    "allow once",
    "always allow",
    "allow all",
    "allow read",
    "allow write",
    "allow bash",
    "allow edit",
    "allow mcp",
    "(y/n)",
    "[y/n]",
];

/// Dangerous command patterns that cause autopilot to disengage.
/// Matched case-insensitively against the current output chunk.
/// Keep patterns specific to avoid false positives from Claude's explanatory text.
const DANGER_PATTERNS: &[&str] = &[
    "rm -rf /",
    "git push --force",
    "git push -f",
    "git reset --hard",
    "chmod 777",
    "dd if=",
    "mkfs",
    "> /dev/",
];

/// Strip ANSI escape sequences and control characters from a byte buffer for keyword scanning.
fn strip_ansi(buf: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(buf);
    String::from_utf8_lossy(&stripped)
        .chars()
        .filter(|c| !c.is_ascii_control() || *c == ' ' || *c == '\n')
        .collect()
}

/// Send a desktop notification via notify-send.
pub fn send_desktop_notification(summary: &str, body: &str) {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("--urgency=normal")
        .arg("-t")
        .arg("10000");

    // Use the bundled icon if we can find it next to the executable
    if let Ok(exe) = std::env::current_exe() {
        let icon = exe.parent().unwrap_or(exe.as_ref()).join("../assets/icon-128.png");
        if let Ok(icon) = icon.canonicalize() {
            cmd.arg("--icon").arg(icon);
        }
    }

    cmd.arg(summary).arg(body);

    match cmd.spawn() {
        Ok(_) => info!("[notify] notification sent: {}", summary),
        Err(e) => warn!("[notify] notify-send failed: {}", e),
    }
}

/// Spawn a proxy thread that reads PTY output, forwards it to a pipe for
/// TerminalView, and scans for keywords to trigger desktop notifications
/// or autopilot auto-accept.
fn spawn_output_scanner(
    mut pty_reader: Box<dyn Read + Send>,
    session_name: Arc<Mutex<String>>,
    autopilot: Arc<AtomicBool>,
    pty_writer: Arc<Mutex<Box<dyn Write + Send>>>,
) -> Box<dyn Read + Send> {
    let (sock_read, sock_write) = std::os::unix::net::UnixStream::pair()
        .expect("Failed to create socket pair");
    sock_read.set_nonblocking(false).ok();
    sock_write.set_nonblocking(false).ok();

    std::thread::spawn(move || {
        let mut writer = sock_write;
        let mut buf = [0u8; 4096];
        let mut tail = String::new();
        let mut last_notified = std::time::Instant::now() - std::time::Duration::from_secs(60);

        loop {
            let n = match pty_reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            if writer.write_all(&buf[..n]).is_err() {
                break;
            }

            let text = strip_ansi(&buf[..n]);
            // Prepend tail from previous chunk to catch keywords split across reads
            let scan = format!("{tail}{text}");

            // Keep last 256 chars for next iteration (char-safe slicing)
            let char_count = scan.chars().count();
            if char_count > 256 {
                tail = scan.chars().skip(char_count - 256).collect();
            } else {
                tail = scan.clone();
            }

            let scan_lower = scan.to_lowercase();
            let chunk_lower = text.to_lowercase();
            trace!("[notify] stripped chunk: {:?}", &scan_lower);

            for keyword in NOTIFY_KEYWORDS {
                if scan_lower.contains(keyword) {
                    let name = session_name.lock().unwrap().clone();

                    if autopilot.load(Ordering::Relaxed) {
                        // Only check the current chunk for danger patterns, not
                        // the historical tail. Claude's explanatory text often
                        // mentions dangerous commands without them being proposed.
                        let has_danger = DANGER_PATTERNS.iter().any(|p| chunk_lower.contains(p));

                        if has_danger {
                            autopilot.store(false, Ordering::Relaxed);
                            info!("[autopilot] dangerous command detected in '{}', disengaging", name);
                            send_desktop_notification(
                                &format!("claudio: {} AUTOPILOT OFF", name),
                                &format!("{} autopilot disengaged - dangerous command detected", name),
                            );
                        } else {
                            // Debug: show what we matched and a snippet of context
                            let snippet: String = scan_lower.chars().rev().take(120).collect::<String>().chars().rev().collect();
                            info!("[autopilot] auto-accepting prompt in '{}', keyword={:?}, snippet={:?}", name, keyword, snippet);
                            send_desktop_notification(
                                &format!("claudio: {} autopilot accepting", name),
                                &format!("keyword: {:?}\nsnippet: ...{}", keyword, snippet),
                            );
                            // Small delay to let the prompt finish rendering
                            std::thread::sleep(std::time::Duration::from_millis(300));
                            if let Ok(mut w) = pty_writer.lock() {
                                // Claude Code uses a TUI selector for permissions;
                                // the default option ("Allow once") is pre-selected,
                                // so pressing Enter (\r) approves it.
                                match w.write_all(b"\r") {
                                    Ok(()) => {
                                        let _ = w.flush();
                                        info!("[autopilot] wrote Enter to PTY for '{}'", name);
                                    }
                                    Err(e) => {
                                        warn!("[autopilot] FAILED to write to PTY for '{}': {}", name, e);
                                        send_desktop_notification(
                                            &format!("claudio: {} autopilot WRITE FAILED", name),
                                            &format!("Error: {}", e),
                                        );
                                    }
                                }
                            } else {
                                warn!("[autopilot] FAILED to lock PTY writer for '{}'", name);
                                send_desktop_notification(
                                    &format!("claudio: {} autopilot LOCK FAILED", name),
                                    "Could not lock PTY writer",
                                );
                            }
                        }
                    } else {
                        // Debounce: at most one notification per 10 seconds
                        if last_notified.elapsed() < std::time::Duration::from_secs(10) {
                            debug!("[notify] keyword {:?} matched but debounced in session '{}'", keyword, name);
                            break;
                        }
                        last_notified = std::time::Instant::now();
                        info!("[notify] keyword {:?} matched in session '{}'", keyword, name);
                        send_desktop_notification(
                            &format!("claudio: {}", name),
                            &format!("{} is requesting your help", name),
                        );
                    }
                    break;
                }
            }
        }
    });

    Box::new(sock_read)
}

pub struct SessionState {
    pub id: String,
    pub name: String,
    pub mode: String,
    pub focused: bool,
    pub busy: bool,
    pub minimized: bool,
    pub shell_mode: bool,
    pub cwd: Option<PathBuf>,
    pub terminal_view: gpui::Entity<TerminalView>,
    pub autopilot: Arc<AtomicBool>,
    voice_writer: Arc<Mutex<Box<dyn Write + Send>>>,
    notify_name: Arc<Mutex<String>>,
}

impl SessionState {
    pub fn new_with_cwd(
        info: SessionInfo,
        cwd: Option<PathBuf>,
        shell_mode: bool,
        command: Option<Vec<String>>,
        socket_path: PathBuf,
        cx: &mut gpui::Context<super::app::ClaudioApp>,
    ) -> Self {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("Failed to create PTY");

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = if let Some(ref argv) = command {
            // External caller supplied a literal argv. Run it via the
            // user's shell so the user gets a shell after the command
            // exits -- matching the claude/exec $SHELL behaviour below.
            let joined = shell_quote_argv(argv);
            let mut c = CommandBuilder::new(&shell);
            c.args(["-c", &format!("{joined}; exec {shell}")]);
            c
        } else if shell_mode {
            let mut c = CommandBuilder::new(&shell);
            c.args(["-c", &format!("exec {}", shell)]);
            c
        } else {
            let mut c = CommandBuilder::new(&shell);
            c.args(["-c", &format!("claude; exec {}", shell)]);
            c
        };
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(ref dir) = cwd {
            cmd.cwd(dir);
        }

        let _child = pair
            .slave
            .spawn_command(cmd)
            .expect("Failed to spawn claude");
        drop(pair.slave);

        let pty_reader = pair.master.try_clone_reader().expect("Failed to clone PTY reader");
        let notify_name = Arc::new(Mutex::new(info.name.clone()));
        let autopilot = Arc::new(AtomicBool::new(false));

        // take_writer() can only be called once; share via Arc<Mutex<>>
        let writer = pair.master.take_writer().expect("Failed to take PTY writer");
        let shared_writer = Arc::new(Mutex::new(writer));

        let reader = spawn_output_scanner(
            pty_reader,
            Arc::clone(&notify_name),
            Arc::clone(&autopilot),
            Arc::clone(&shared_writer),
        );
        let kb_writer = SharedWriter {
            inner: Arc::clone(&shared_writer),
        };

        let config = TerminalConfig {
            font_family: "DejaVu Sans Mono".into(),
            font_size: gpui::px(14.0),
            cols: 80,
            rows: 24,
            scrollback: 10000,
            line_height_multiplier: 1.0,
            padding: gpui::Edges::all(gpui::px(4.0)),
            colors: default_palette(),
        };

        let master = Arc::new(Mutex::new(pair.master));
        let master_resize = Arc::clone(&master);
        let exit_session_id = info.id.clone();
        let terminal_view = cx.new(|cx| {
            TerminalView::new(kb_writer, reader, config, cx)
                .with_resize_callback(move |cols: usize, rows: usize| {
                    if let Ok(m) = master_resize.lock() {
                        let _ = m.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                })
                .with_exit_callback(move |_window, _cx| {
                    let socket = socket_path.clone();
                    let session_id = exit_session_id.clone();
                    std::thread::spawn(move || {
                        let _ = ipc_bridge::send_command(
                            &socket,
                            &Request::Kill { session: session_id },
                        );
                    });
                })
        });

        info!("Created terminal for session '{}'", info.name);

        Self {
            id: info.id,
            name: info.name,
            mode: info.mode,
            focused: info.focused,
            busy: info.busy,
            minimized: false,
            shell_mode,
            cwd,
            terminal_view,
            autopilot,
            voice_writer: shared_writer,
            notify_name,
        }
    }

    /// Update the session name, keeping the notification thread in sync.
    pub fn set_name(&mut self, name: String) {
        self.name = name.clone();
        *self.notify_name.lock().unwrap() = name;
    }

    /// Write raw text to the PTY without appending a carriage return.
    pub fn write_raw(&self, text: &str) {
        if let Ok(mut w) = self.voice_writer.lock() {
            if let Err(e) = w.write_all(text.as_bytes()) {
                warn!("Failed to write to PTY: {e}");
            }
            let _ = w.flush();
        }
    }
}

/// Quote each argv element for a single `sh -c` line.
fn shell_quote_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            let escaped = a.replace('\'', "'\\''");
            format!("'{escaped}'")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_palette() -> ColorPalette {
    ColorPalette::builder()
        .background(0x1e, 0x1e, 0x2e)
        .foreground(0xcd, 0xd6, 0xf4)
        .cursor(0xf5, 0xe0, 0xdc)
        .black(0x45, 0x47, 0x5a)
        .red(0xf3, 0x8b, 0xa8)
        .green(0xa6, 0xe3, 0xa1)
        .yellow(0xf9, 0xe2, 0xaf)
        .blue(0x89, 0xb4, 0xfa)
        .magenta(0xf5, 0xc2, 0xe7)
        .cyan(0x94, 0xe2, 0xd5)
        .white(0xba, 0xc2, 0xde)
        .bright_black(0x58, 0x5b, 0x70)
        .bright_red(0xf3, 0x8b, 0xa8)
        .bright_green(0xa6, 0xe3, 0xa1)
        .bright_yellow(0xf9, 0xe2, 0xaf)
        .bright_blue(0x89, 0xb4, 0xfa)
        .bright_magenta(0xf5, 0xc2, 0xe7)
        .bright_cyan(0x94, 0xe2, 0xd5)
        .bright_white(0xa6, 0xad, 0xc8)
        .build()
}
