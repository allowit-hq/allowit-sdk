//! The `allowit` command. Argument parsing, terminal output and rail adapters
//! live here; engine requests and service composition come from
//! `allowit-sdk-engine`, which has no terminal I/O.
pub use allowit_native as native;
pub use allowit_sdk::admin;
pub use allowit_sdk_engine::{VERSION, error, wallet};
pub(crate) use allowit_sdk_engine::{
    client, config, output, request, routing, skill, typed_request,
};
pub use allowit_stellar_wallet as stellar;
mod agents;
mod commands;
mod ctx;
mod exit;
mod gateway;
mod settings;
mod term;
mod vendors;
use crate::{config::env_value, error::Error, exit::Exit, term::Out};
use serde_json::{Value, json};
use std::io::Write;
pub use vendors::solana::handoff;
pub use vendors::stellar::{approval as stellar_approval, owner as stellar_owner};

/// Every command path, hidden aliases included, for the help snapshot tests.
#[doc(hidden)]
pub fn command_paths() -> Vec<Vec<String>> {
    commands::paths()
}

/// Runs the command line (without the program name) and returns the exit code.
pub fn run(args: Vec<String>) -> i32 {
    let mut argv = vec!["allowit".to_owned()];
    argv.extend(args);
    // Before parsing, so that parse errors honor them too.
    let flags: Vec<&str> = argv
        .iter()
        .take_while(|a| a.as_str() != "--")
        .map(String::as_str)
        .collect();
    let json = flags.contains(&"--json")
        || flags
            .windows(2)
            .any(|w| w[0] == "--output" && w[1] == "json")
        || flags.contains(&"--output=json");
    let no_color = flags.contains(&"--no-color");
    match commands::parse(&argv, term::color_allowed(no_color, true)) {
        commands::Parsed::Print(text) => {
            let _ = std::io::stdout().write_all(text.as_bytes());
            0
        }
        commands::Parsed::Error(e) => {
            let code = Exit::Usage.code();
            let mut message = e.render().to_string();
            if e.kind() == clap::error::ErrorKind::InvalidSubcommand {
                let commands = commands::available_commands(&argv);
                if !commands.is_empty() {
                    message = format!(
                        "{}\n\nCommands: {}\n",
                        message.trim_end(),
                        commands.join(", ")
                    );
                }
            }
            if json {
                let text = strip_ansi(&message);
                let first = text
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("usage error")
                    .trim_start_matches("error: ");
                let value = json!({"error":{"code":Exit::Usage.name(),"exit":code,"message":first,"request_id":Value::Null}});
                let _ = writeln!(std::io::stderr(), "{value}");
            } else {
                let _ = std::io::stderr().write_all(message.as_bytes());
            }
            code
        }
        commands::Parsed::Run(cli) => execute(*cli),
    }
}

fn strip_ansi(text: &str) -> String {
    regex::Regex::new("\x1b\\[[0-9;]*m")
        .map(|re| re.replace_all(text, "").into_owned())
        .unwrap_or_else(|_| text.to_owned())
}

fn execute(cli: commands::Cli) -> i32 {
    let global = cli.global;
    let mut out = Out::new(
        global.json_output(),
        global.quiet,
        global.verbose,
        global.no_color,
    );
    out.live = true;
    let mut cx = ctx::Ctx::new(global, out);
    let Some(command) = cli.command else {
        return 0;
    };
    // Commands that read no rail never resolve one, so a broken rail setting
    // cannot change their output.
    let network = if commands::is_local(&command) {
        None
    } else {
        cx.rail_opt().ok().flatten().map(|r| r.id.to_owned())
    };
    if let Some(n) = &network
        && cx.global.verbose
        && let Ok(dir) = cx.dir()
    {
        cx.out.detail(format!("rail {n} config {}", dir.display()));
    }
    let result = routing::with_network(network, || commands::dispatch(command, &mut cx));
    finish(cx.out, result)
}

fn finish(mut out: Out, result: error::Result<i32>) -> i32 {
    let code = match result {
        Ok(code) => code,
        Err(e) => {
            render_error(&mut out, &e);
            e.code
        }
    };
    let stdout = std::mem::take(&mut out.stdout);
    let stdout = if out.raw {
        stdout
    } else {
        redact(safe_json_output(stdout))
    };
    let stdout = out.rendered_stdout(stdout);
    let _ = std::io::stdout().write_all(stdout.as_bytes());
    let _ = std::io::stderr().write_all(redact(std::mem::take(&mut out.stderr)).as_bytes());
    code
}

fn render_error(out: &mut Out, e: &Error) {
    if out.json {
        let value = json!({"error":{
            "code": Exit::from_code(e.code).name(),
            "exit": e.code,
            "message": e.message,
            "request_id": out.request_id,
        }});
        out.stderr += &format!("{value}\n");
    } else {
        let label = out.error_label();
        out.stderr += &format!("{label} {}\n", e.message);
    }
}

fn safe_json_output(stdout: String) -> String {
    if serde_json::from_str::<Value>(&stdout).is_ok() {
        // Escape display controls within JSON without changing its value.
        stdout
            .chars()
            .map(|c| {
                if ('\u{7f}'..'\u{a0}').contains(&c)
                    || ('\u{202a}'..='\u{202e}').contains(&c)
                    || ('\u{2066}'..='\u{2069}').contains(&c)
                {
                    format!("\\u{:04x}", c as u32)
                } else {
                    c.to_string()
                }
            })
            .collect()
    } else {
        stdout
    }
}

/// Removes the engine tokens from any output.
pub(crate) fn redact(s: String) -> String {
    output::clean(
        output::clean(s, &env_value("ALLOWIT_TOKEN")),
        &env_value(gateway::ACCOUNT_TOKEN_ENV),
    )
}

/// Writes pending stderr now, before a request can leave the process.
pub(crate) fn flush_stderr(stderr: &mut String) {
    let _ = std::io::stderr().write_all(redact(std::mem::take(stderr)).as_bytes());
}

#[cfg(test)]
mod display_tests {
    #[test]
    fn json_display_controls_are_escaped() {
        let original = serde_json::json!({"prompt":"source\u{202e} text\u{0085}"});
        let encoded = super::safe_json_output(original.to_string());
        assert!(!encoded.contains('\u{202e}'));
        assert!(!encoded.contains('\u{0085}'));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
            original
        );
    }
}
