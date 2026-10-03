//! Install a script that other processes will `exec`, without the
//! "Text file busy (os error 26)" flake.
//!
//! Tests run on many threads in one process. A thread that spawns a child
//! forks the whole descriptor table, so a script another thread has just
//! written (and still has open for writing) can sit in a child's table until
//! that child execs. Executing the script in that window fails with ETXTBSY.
//! `install_executable` writes under a temporary name, syncs and closes it,
//! renames it into place, then probes it until `exec` succeeds, which only
//! happens once no forked child still holds a write descriptor. The writer's
//! own descriptor is already closed, so no later fork can inherit it.
#![allow(dead_code)]
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_ARGUMENT: &str = "--etxtbsy-probe";
const ETXTBSY: i32 = 26;

/// Write `script` (which must start with a `#!` line) as an executable at `path`.
/// The script answers `--etxtbsy-probe` with an immediate success, before any
/// of its own logic runs, so probing never touches the test's recorded state.
pub fn install_executable(path: &Path, script: &str) {
    let (shebang, rest) = script
        .split_once('\n')
        .expect("a script needs a shebang line");
    assert!(shebang.starts_with("#!"), "a script needs a shebang line");
    let script = format!("{shebang}\n[ \"${{1:-}}\" = {PROBE_ARGUMENT} ] && exit 0\n{rest}");

    let parent = path.parent().expect("script directory");
    let name = path.file_name().expect("script name").to_string_lossy();
    let temporary = parent.join(format!(".{name}.installing"));
    {
        let mut file = fs::File::create(&temporary).expect("create script");
        file.write_all(script.as_bytes()).expect("write script");
        file.set_permissions(fs::Permissions::from_mode(0o755))
            .expect("chmod script");
        file.sync_all().expect("sync script");
    }
    fs::rename(&temporary, path).expect("publish script");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match Command::new(path)
            .arg(PROBE_ARGUMENT)
            .current_dir("/")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Err(error) if error.raw_os_error() == Some(ETXTBSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => {
                result.expect("probe script");
                return;
            }
        }
    }
}
