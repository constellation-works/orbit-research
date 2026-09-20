//! Resolve terminal policy once, before command execution.
use clap::ValueEnum;
use std::io::IsTerminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum OutputMode {
    Auto,
    Table,
    Json,
    Ndjson,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Table,
    Plain,
    Json,
    Ndjson,
}

pub(crate) struct OutputSink {
    pub(crate) mode: Mode,
    pub(crate) width: usize,
    pub(crate) color: bool,
    pub(crate) suppress_uniform: bool,
    pub(crate) interactive: bool,
}

impl OutputSink {
    pub(crate) fn from_process(requested: OutputMode) -> Self {
        let interactive = std::io::stdout().is_terminal();
        let width = if interactive {
            std::env::var("COLUMNS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|w| *w > 0)
                .or_else(terminal_width)
                .unwrap_or(0)
        } else {
            0
        };
        let color = std::env::var("NO_COLOR").unwrap_or_default().is_empty()
            && std::env::var("TERM").unwrap_or_default() != "dumb";
        Self::resolve(requested, interactive, width, color)
    }

    pub(crate) fn resolve(
        requested: OutputMode,
        interactive: bool,
        width: usize,
        color: bool,
    ) -> Self {
        let mode = match requested {
            OutputMode::Auto if interactive => Mode::Table,
            OutputMode::Auto => Mode::Plain,
            OutputMode::Table => Mode::Table,
            OutputMode::Json => Mode::Json,
            OutputMode::Ndjson => Mode::Ndjson,
        };
        Self {
            mode,
            width: if interactive { width } else { 0 },
            color: interactive && color && mode == Mode::Table,
            suppress_uniform: requested == OutputMode::Auto && interactive,
            interactive,
        }
    }

    pub(crate) fn machine(&self) -> bool {
        matches!(self.mode, Mode::Json | Mode::Ndjson)
    }
}

/// Recover only the global output preference when clap rejects the invocation.
/// Help/version are printed by clap itself and never wrapped as data.
pub(crate) fn error_format(args: &[std::ffi::OsString]) -> OutputMode {
    let mut requested = None;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--format" {
            requested = args.next().and_then(|s| s.to_str());
        } else if let Some(value) = arg.to_str().and_then(|s| s.strip_prefix("--format=")) {
            requested = Some(value);
        }
    }
    requested
        .map(str::to_owned)
        .or_else(|| std::env::var("ORBIT_RESEARCH_FORMAT").ok())
        .and_then(|s| OutputMode::from_str(&s, false).ok())
        .unwrap_or(OutputMode::Auto)
}

#[cfg(unix)]
fn terminal_width() -> Option<usize> {
    let mut size = std::mem::MaybeUninit::<libc::winsize>::zeroed();
    // SAFETY: ioctl writes only the allocated winsize; read it only on success.
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) } == 0 {
        let width = unsafe { size.assume_init() }.ws_col;
        (width > 0).then_some(usize::from(width))
    } else {
        None
    }
}

#[cfg(not(unix))]
fn terminal_width() -> Option<usize> {
    None
}
