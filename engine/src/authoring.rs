//! Portable source validation and bounded host compilation through the exact Core dependency.
use crate::error::{Error, Result};
use allowit_sdk::policy_source;
use std::{
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_SOURCE_BYTES: usize = 65_536;
const MAX_OUTPUT_BYTES: u64 = 1_048_576;

/// Caller-owned host tool bindings. The result is a local type-check, never publication authority.
pub struct HostTools {
    pub python: PathBuf,
    pub rustc: PathBuf,
    pub timeout: Duration,
}
impl Default for HostTools {
    fn default() -> Self {
        Self {
            python: "python3".into(),
            rustc: "rustc".into(),
            timeout: Duration::from_secs(30),
        }
    }
}
pub fn check(source: &str) -> Result<policy_source::SourceShape> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::usage("Portable policy source exceeds 65536 bytes"));
    }
    policy_source::source_shape(source).map_err(Error::usage)
}

/// Compile structurally admitted source against the exact embedded portable Std library.
/// Missing tools, time/output limits and compilation failures fail closed. Nothing is published.
#[cfg(unix)]
pub fn compile(source: &str, tools: &HostTools) -> Result<()> {
    use std::os::unix::process::CommandExt;
    check(source)?;
    if tools.timeout.is_zero() || tools.timeout > Duration::from_secs(60) {
        return Err(Error::config(
            "Host compiler timeout must be within 60 seconds",
        ));
    }
    let stage = tempfile::tempdir().map_err(io_error)?;
    let root = stage.path();
    std::fs::create_dir_all(root.join("scripts")).map_err(io_error)?;
    std::fs::create_dir_all(root.join("crates/allowit/src")).map_err(io_error)?;
    std::fs::create_dir(root.join("tmp")).map_err(io_error)?;
    for (path, bytes) in [
        ("scripts/policy_source.py", policy_source::PYTHON_HELPER),
        ("scripts/LICENSE", policy_source::PYTHON_HELPER_TERMS),
        ("crates/allowit/src/lib.rs", policy_source::PORTABLE_LIB_RS),
        (
            "crates/allowit/src/core.rs",
            policy_source::PORTABLE_CORE_RS,
        ),
        ("crates/allowit/LICENSE", policy_source::PORTABLE_TERMS),
        ("policy.rs", source),
    ] {
        std::fs::write(root.join(path), bytes.as_bytes()).map_err(io_error)?;
    }
    let digest = allowit_sdk::digest(source.as_bytes());
    let mut command = Command::new(&tools.python);
    command
        .args(["-I", "-c", DRIVER])
        .arg(root)
        .arg(&digest)
        .current_dir(root)
        .env("RUSTC", &tools.rustc)
        .env("TMPDIR", root.join("tmp"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // Only async-signal-safe syscalls run between fork and exec.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 30,
                rlim_max: 30,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let limit = libc::rlimit {
                rlim_cur: 64 * 1024 * 1024,
                rlim_max: 64 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .map_err(|_| Error::config("Cannot start the Core host compiler; Python is required"))?;
    let mut process = CompilerProcess {
        child,
        active: true,
    };
    let exceeded = Arc::new(AtomicBool::new(false));
    let capture = |pipe: Box<dyn Read + Send>| {
        let exceeded = exceeded.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_OUTPUT_BYTES {
                exceeded.store(true, Ordering::Relaxed);
            }
            Ok::<_, std::io::Error>(bytes)
        })
    };
    let stdout =
        capture(Box::new(process.child.stdout.take().ok_or_else(|| {
            Error::config("Cannot capture compiler output")
        })?));
    let stderr =
        capture(Box::new(process.child.stderr.take().ok_or_else(|| {
            Error::config("Cannot capture compiler diagnostics")
        })?));
    let started = Instant::now();
    let mut stopped = None;
    let mut exited = None;
    let status = loop {
        if exited.is_none() {
            exited = process.child.try_wait().map_err(io_error)?;
        }
        // A descendant can retain the pipes after its parent exits. Keep the same
        // wall/output limit until the complete process output is available.
        if let Some(status) = exited
            && stdout.is_finished()
            && stderr.is_finished()
        {
            process.active = false;
            break status;
        }
        if started.elapsed() >= tools.timeout || exceeded.load(Ordering::Relaxed) {
            stopped = Some(if exceeded.load(Ordering::Relaxed) {
                "Core host compiler output exceeds its limit"
            } else {
                "Core host compilation timed out"
            });
            unsafe {
                libc::kill(-(process.child.id() as libc::pid_t), libc::SIGKILL);
            }
            let status = process.child.wait().map_err(io_error)?;
            process.active = false;
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout
        .join()
        .map_err(|_| Error::config("Cannot read compiler output"))?
        .map_err(io_error)?;
    let stderr = stderr
        .join()
        .map_err(|_| Error::config("Cannot read compiler diagnostics"))?
        .map_err(io_error)?;
    if let Some(message) = stopped {
        return Err(Error::config(message));
    }
    if exceeded.load(Ordering::Relaxed) {
        return Err(Error::config("Core host compiler output exceeds its limit"));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        let message = format!("Portable host compilation failed: {}", detail.trim());
        return Err(if status.code() == Some(2) {
            Error::usage(message)
        } else {
            Error::config(message)
        });
    }
    if stdout != format!("allowit-host-compile-v1 {digest}\n").as_bytes() {
        return Err(Error::config(
            "Core host compilation did not confirm the exact source",
        ));
    }
    Ok(())
}
#[cfg(unix)]
struct CompilerProcess {
    child: Child,
    active: bool,
}
#[cfg(unix)]
impl Drop for CompilerProcess {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                libc::kill(-(self.child.id() as libc::pid_t), libc::SIGKILL);
            }
        }
        let _ = self.child.wait();
    }
}
#[cfg(not(unix))]
pub fn compile(_source: &str, _tools: &HostTools) -> Result<()> {
    Err(Error::unsupported(
        "Bounded host policy compilation requires a POSIX runtime",
    ))
}
fn io_error(_: std::io::Error) -> Error {
    Error::config("Cannot prepare or observe the private host compilation")
}

#[cfg(unix)]
const DRIVER: &str = r#"
import hashlib, importlib.util, os, shutil, sys
from pathlib import Path
root=Path(sys.argv[1]); source=root/'policy.rs'
if shutil.which(os.environ['RUSTC']) is None:
    print('The Core host compile requires rustc',file=sys.stderr);sys.exit(3)
if hashlib.sha256(source.read_bytes()).hexdigest()!=sys.argv[2]:
    print('The original source changed before compilation',file=sys.stderr);sys.exit(3)
spec=importlib.util.spec_from_file_location('allowit_core_policy_source',root/'scripts/policy_source.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
try:
    module.checked_code(source.read_text(encoding='utf-8'))
    module.host_compile(source)
except ValueError as error:
    print(error,file=sys.stderr);sys.exit(2)
print('allowit-host-compile-v1 '+sys.argv[2])
"#;
