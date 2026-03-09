use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use gpui;
use gpui::AppContext;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tracing::{info, warn};

use std::path::PathBuf;

use crate::ipc::protocol::SessionInfo;

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
const NOTIFY_KEYWORDS: &[&str] = &[
    "Do you want to proceed?",
    "approve",
    "permission",
    "Allow",
    "Deny",
    "(y/n)",
    "(Y/n)",
    "[Y/n]",
    "[y/N]",
    "Press Enter",
    "waiting for",
];

/// Strip ANSI escape sequences from a byte buffer for keyword scanning.
fn strip_ansi(buf: &[u8]) -> String {
    let s = String::from_utf8_lossy(buf);
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some(']') => {
                    // OSC sequence: skip until BEL or ST (\x1b\\)
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    // CSI / other: skip until alphabetic or ~
                    while let Some(c) = chars.next() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
            }
        } else if ch == '\x07' || ch == '\x08' {
            // Skip BEL and backspace
        } else {
            out.push(ch);
        }
    }
    out
}

/// Spawn a proxy thread that reads PTY output, forwards it to a pipe for
/// TerminalView, and scans for keywords to trigger desktop notifications.
fn spawn_output_scanner(
    mut pty_reader: Box<dyn Read + Send>,
    session_name: String,
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

            // Debounce: at most one notification per 30 seconds
            if last_notified.elapsed() < std::time::Duration::from_secs(30) {
                continue;
            }

            for keyword in NOTIFY_KEYWORDS {
                if scan.contains(keyword) {
                    last_notified = std::time::Instant::now();
                    let _ = notify_rust::Notification::new()
                        .summary(&format!("claudio: {}", session_name))
                        .body(&format!("{} needs your help", session_name))
                        .timeout(5000)
                        .show();
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
    pub terminal_view: gpui::Entity<TerminalView>,
    voice_writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl SessionState {
    pub fn new_with_cwd(
        info: SessionInfo,
        cwd: Option<PathBuf>,
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

        let mut cmd = CommandBuilder::new("claude");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        let _child = pair
            .slave
            .spawn_command(cmd)
            .expect("Failed to spawn claude");
        drop(pair.slave);

        let pty_reader = pair.master.try_clone_reader().expect("Failed to clone PTY reader");
        let reader = spawn_output_scanner(pty_reader, info.name.clone());

        // take_writer() can only be called once; share via Arc<Mutex<>>
        let writer = pair.master.take_writer().expect("Failed to take PTY writer");
        let shared_writer = Arc::new(Mutex::new(writer));
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
        let terminal_view = cx.new(|cx| {
            TerminalView::new(kb_writer, reader, config, cx).with_resize_callback(
                move |cols: usize, rows: usize| {
                    if let Ok(m) = master_resize.lock() {
                        let _ = m.resize(PtySize {
                            rows: rows as u16,
                            cols: cols as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                },
            )
        });

        info!("Created terminal for session '{}'", info.name);

        Self {
            id: info.id,
            name: info.name,
            mode: info.mode,
            focused: info.focused,
            busy: info.busy,
            minimized: false,
            terminal_view,
            voice_writer: shared_writer,
        }
    }

    /// Write voice transcription to the PTY.
    pub fn write_transcription(&self, text: &str) {
        if let Ok(mut w) = self.voice_writer.lock() {
            let input = format!("{text}\r");
            if let Err(e) = w.write_all(input.as_bytes()) {
                warn!("Failed to write transcription to PTY: {e}");
            }
            let _ = w.flush();
        }
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
