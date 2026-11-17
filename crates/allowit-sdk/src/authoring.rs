//! Portable source validation and bounded host compilation through the exact Core dependency.
use crate::error::{Error, Result};
use allowit_core::policy_source;
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

const MAX_OUTPUT_BYTES: u64 = 1_048_576;

/// Caller-owned host tool bindings. The result is a local type-check, never publication authority.
pub struct HostTools {
    pub rustc: PathBuf,
    /// Wall-clock limit of the whole compilation, at most 60 seconds.
    pub timeout: Duration,
}
impl Default for HostTools {
    fn default() -> Self {
        Self {
            rustc: "rustc".into(),
            timeout: Duration::from_secs(30),
        }
    }
}
pub use allowit_core::vendor::authoring::{canonical_digest, check, portable_identity};

/// The server functions that Core declares in the `allowit` namespace. No contract can call
/// one: a server evaluates the policy that names one. Core keeps this list in its source gate
/// and exports no copy, so a new Core server function must be added here. Until then the
/// server profile refuses a policy that names it, as it refuses every function outside its
/// registry.
pub const SERVER_FUNCTIONS: [(&str, &str); 7] = [
    ("allowit", "price"),
    ("allowit", "new_decision_context"),
    ("allowit", "decision"),
    ("allowit", "request_screening"),
    ("allowit", "fetch"),
    ("allowit", "internet_search"),
    ("allowit", "marketplace_search"),
];

/// The checked shape of server policy source, and the server functions that it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerShape {
    pub shape: policy_source::SourceShape,
    /// Each registered function that the source names, as `namespace::function`, in registry
    /// order. A source that names one has a part that only a server can run.
    pub functions: Vec<String>,
}

/// Check server policy source as a server of this build checks it: with Core's server policy
/// profile, against [`SERVER_FUNCTIONS`] and the evidence and package functions that this build
/// registers ([`crate::plugins::policy_registry`]). Beside the portable vocabulary, that
/// profile admits a call of a registered function and the text of an owner param, read with
/// `as_str()`. A server decides which of the functions its build fills.
pub fn check_server(source: &str) -> Result<ServerShape> {
    let registry: Vec<(&str, &str)> = SERVER_FUNCTIONS
        .into_iter()
        .chain(crate::plugins::policy_registry(&SERVER_FUNCTIONS)?.host_calls)
        .collect();
    let shape =
        policy_source::source_shape_with_host_calls(source, &registry).map_err(Error::usage)?;
    // The gate refuses a function that the registry does not hold. So the source names a
    // function when the registry without it no longer admits the source.
    let functions = registry
        .iter()
        .filter(|function| {
            let without: Vec<_> = registry.iter().filter(|other| other != function).collect();
            let without: Vec<(&str, &str)> = without.into_iter().copied().collect();
            policy_source::source_shape_with_host_calls(source, &without).is_err()
        })
        .map(|(namespace, name)| format!("{namespace}::{name}"))
        .collect();
    Ok(ServerShape { shape, functions })
}

/// The host crate of one policy: a `no_std` crate whose only dependency is `allowit`.
const POLICY_HOST: &str = "#![no_std]\n#[path = \"policy.rs\"]\nmod policy;\npub const EXECUTE: fn(&allowit::AllowitContext) -> allowit::Result = policy::execute;\n";
/// Flags of every compilation. Function pointer comparisons depend on code layout, so the lint is
/// forbidden and no source attribute can lower its level.
const RUSTC_FLAGS: [&str; 5] = [
    "--edition=2024",
    "-C",
    "overflow-checks=on",
    "-F",
    "unpredictable_function_pointer_comparisons",
];

/// Compile structurally admitted source against the exact embedded portable Std library.
/// The Rust source gate runs first. Then `rustc` builds the portable library with only its `std`
/// feature and the policy as a module of a `no_std` crate, with overflow checks on.
/// Missing tools, time/output limits and compilation failures fail closed. Nothing is published.
#[cfg(unix)]
pub fn compile(source: &str, tools: &HostTools) -> Result<()> {
    check(source)?;
    build(source, tools)
}

/// [`compile`] for server policy source: [`check_server`] runs first. The portable Std library
/// declares every server function, so the policy type-checks as it does in a server's build.
#[cfg(unix)]
pub fn compile_server(source: &str, tools: &HostTools) -> Result<()> {
    check_server(source)?;
    build(source, tools)
}

/// Build the admitted `source` against the embedded portable Std library.
#[cfg(unix)]
fn build(source: &str, tools: &HostTools) -> Result<()> {
    if tools.timeout.is_zero() || tools.timeout > Duration::from_secs(60) {
        return Err(Error::config(
            "Host compiler timeout must be within 60 seconds",
        ));
    }
    let deadline = Instant::now() + tools.timeout;
    let stage = tempfile::tempdir().map_err(io_error)?;
    let root = stage.path();
    for directory in ["allowit", "out", "tmp"] {
        std::fs::create_dir(root.join(directory)).map_err(io_error)?;
    }
    for (path, bytes) in [
        ("allowit/lib.rs", policy_source::PORTABLE_LIB_RS),
        ("allowit/core.rs", policy_source::PORTABLE_CORE_RS),
        ("allowit/LICENSE", policy_source::PORTABLE_TERMS),
        ("policy.rs", source),
        ("policy_host.rs", POLICY_HOST),
    ] {
        std::fs::write(root.join(path), bytes.as_bytes()).map_err(io_error)?;
    }
    let library = [
        "--crate-type=rlib",
        "--crate-name=allowit",
        "--cfg",
        "feature=\"std\"",
        "allowit/lib.rs",
        "-o",
        "out/liballowit.rlib",
    ];
    let policy = [
        "--crate-type=rlib",
        "--crate-name=policy_host",
        "--extern",
        "allowit=out/liballowit.rlib",
        "policy_host.rs",
        "-o",
        "out/libpolicy_host.rlib",
    ];
    for (arguments, output, authored) in [
        (library, "out/liballowit.rlib", false),
        (policy, "out/libpolicy_host.rlib", true),
    ] {
        let (status, stderr) = run(tools, root, &arguments, deadline)?;
        if !status.success() {
            let detail = String::from_utf8_lossy(&stderr);
            let message = format!("Portable host compilation failed: {}", detail.trim());
            // rustc exits with 1 for errors in the compiled source.
            return Err(if authored && status.code() == Some(1) {
                Error::usage(message)
            } else {
                Error::config(message)
            });
        }
        // A tool that exits without the compiled library did not compile the source.
        if !std::fs::metadata(root.join(output)).is_ok_and(|file| file.is_file() && file.len() > 0)
        {
            return Err(Error::config(
                "Core host compilation did not confirm the exact source",
            ));
        }
    }
    Ok(())
}

/// Run one bounded `rustc` process in the private stage and return its status and diagnostics.
#[cfg(unix)]
fn run(
    tools: &HostTools,
    root: &std::path::Path,
    arguments: &[&str],
    deadline: Instant,
) -> Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(&tools.rustc);
    // The compiler receives only public runtime/toolchain paths, never provider or signer environment.
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("LANG", "C")
        .env("LC_ALL", "C");
    for (name, suffix) in [("RUSTUP_HOME", ".rustup"), ("CARGO_HOME", ".cargo")] {
        if let Some(path) = std::env::var_os(name)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(suffix)))
        {
            command.env(name, path);
        }
    }
    if let Some(toolchain) = std::env::var_os("RUSTUP_TOOLCHAIN") {
        command.env("RUSTUP_TOOLCHAIN", toolchain);
    }
    command
        .args(RUSTC_FLAGS)
        .args(arguments)
        .current_dir(root)
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
        .map_err(|_| Error::config("Cannot start the Core host compiler; rustc is required"))?;
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
        if Instant::now() >= deadline || exceeded.load(Ordering::Relaxed) {
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
    stdout
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
    Ok((status, stderr))
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
#[cfg(not(unix))]
pub fn compile_server(_source: &str, _tools: &HostTools) -> Result<()> {
    Err(Error::unsupported(
        "Bounded host policy compilation requires a POSIX runtime",
    ))
}
fn io_error(_: std::io::Error) -> Error {
    Error::config("Cannot prepare or observe the private host compilation")
}
