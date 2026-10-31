//! The command tree. One folder per command group and one file per verb;
//! each verb file holds its clap arguments and its `run`. Command files reach
//! a chain only through `vendors::rail`.
mod call;
mod claude;
mod codex;
mod compiler;
mod completion;
mod config;
mod curl;
mod eval;
mod exec;
mod guide;
mod history;
mod http;
mod json;
mod keys;
mod legacy;
mod lsp;
mod mcp;
mod policy;
mod rail;
mod setup;
mod skills;
mod status;
mod tx;
mod version;
mod wallet;
mod whoami;

use crate::{
    VERSION,
    ctx::Ctx,
    error::Result,
    vendors::{SignWith, Signing},
};
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, time::Duration};

/// Help layout: about, usage, examples, then commands and options.
const TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}\n\n{before-help}{all-args}{after-help}";

/// The help template of a node with examples. The examples are literal
/// template text, which clap never wraps, so a long example stays one line
/// that copies whole.
fn examples_template(examples: &str) -> String {
    format!(
        "{{about-with-newline}}\n{{usage-heading}} {{usage}}\n\n{examples}\n\n{{all-args}}{{after-help}}"
    )
}

const ROOT_EXAMPLES: &str = "\
Examples:
  allowit rail use stellar:testnet
  allowit keys generate agent --fund
  allowit setup --agent agent
  allowit exec --op transfer --to GDEST… --amount XLM:5
  allowit status pay-0001 --wait 60s";

const EXIT_CODES: &str = "\
Exit codes:
  0 ok, 1 internal error, 2 usage error, 3 configuration or not supported on
  this rail, 4 rejected by the contract, 5 uncertain (retry with the same ID),
  6 replayed, 10 owner signature needed, 11 waiting for owner input,
  12 pending, 20 denied by the policy, 130 interrupted";

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    Text,
    Json,
}

/// Flags that every command accepts.
#[derive(Args, Clone, Debug, Default)]
#[command(next_help_heading = "Global options")]
pub(crate) struct Global {
    /// Rail as CAIP-2 or alias: stellar:testnet, solana:devnet, tempo:testnet
    #[arg(
        long,
        short = 'n',
        visible_alias = "network",
        env = "ALLOWIT_RAIL",
        hide_env_values = true,
        global = true,
        value_name = "RAIL"
    )]
    pub rail: Option<String>,
    /// Policy wallet address: C… on Stellar, a vault on Solana, 0x… on Tempo
    #[arg(
        long,
        env = "ALLOWIT_WALLET",
        hide_env_values = true,
        global = true,
        value_name = "ADDR"
    )]
    pub wallet: Option<String>,
    /// Output format
    #[arg(long, value_enum, global = true, value_name = "FORMAT")]
    pub output: Option<Format>,
    /// Same as --output json
    #[arg(long, global = true)]
    pub json: bool,
    /// Confirm destructive actions without a prompt
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
    /// Print no progress on stderr
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    /// Print the resolved rail, wallet and config directory on stderr
    #[arg(short, long, global = true)]
    pub verbose: bool,
    /// Disable color (NO_COLOR also disables it)
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Config directory [default: $XDG_CONFIG_HOME/allowit or ~/.config/allowit]
    #[arg(
        long,
        env = "ALLOWIT_CONFIG_DIR",
        hide_env_values = true,
        global = true,
        value_name = "DIR"
    )]
    pub config_dir: Option<PathBuf>,
    /// RPC URL override for the selected rail
    #[arg(long, global = true, value_name = "URL")]
    pub rpc_url: Option<String>,
    /// True when `--rail` is on the command line, not from `ALLOWIT_RAIL`.
    #[arg(skip)]
    pub rail_flag: bool,
    /// True when `--wallet` is on the command line, not from `ALLOWIT_WALLET`.
    #[arg(skip)]
    pub wallet_flag: bool,
}

impl Global {
    /// JSON with `--json` or `--output json`; also with `NO_DNA` unless
    /// `--output text` asks for text.
    pub(crate) fn json_output(&self) -> bool {
        self.json
            || match self.output {
                Some(format) => format == Format::Json,
                None => crate::term::no_dna(),
            }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "allowit",
    about = "AllowIt: policy wallets for agents. Go on. On your terms.",
    version = VERSION,
    before_help = ROOT_EXAMPLES,
    after_help = EXIT_CODES,
    disable_version_flag = false
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    #[command(before_help = setup::EXAMPLES)]
    Setup(setup::Args),
    #[command(before_help = whoami::EXAMPLES)]
    Whoami(whoami::Args),
    #[command(subcommand, before_help = keys::EXAMPLES)]
    Keys(keys::Command),
    #[command(subcommand, before_help = rail::EXAMPLES)]
    Rail(rail::Command),
    #[command(subcommand, before_help = config::EXAMPLES)]
    Config(config::Command),
    #[command(subcommand, before_help = wallet::EXAMPLES)]
    Wallet(wallet::Command),
    #[command(subcommand, before_help = policy::EXAMPLES)]
    Policy(policy::Command),
    #[command(before_help = exec::EXAMPLES)]
    Exec(exec::Args),
    #[command(before_help = eval::EXAMPLES)]
    Eval(eval::Args),
    #[command(before_help = status::EXAMPLES)]
    Status(status::Args),
    #[command(before_help = history::EXAMPLES)]
    History(history::Args),
    #[command(subcommand)]
    Tx(tx::Command),
    #[command(subcommand)]
    Skills(skills::Command),
    #[command(before_help = call::EXAMPLES)]
    Call(call::Args),
    #[command(alias = "wget")]
    Curl(curl::Args),
    #[command(alias = "fetch")]
    Http(http::Args),
    Mcp(mcp::Args),
    Claude(claude::Args),
    Codex(codex::Args),
    #[command(before_help = lsp::EXAMPLES)]
    Lsp(lsp::Args),
    #[command(before_help = completion::EXAMPLES)]
    Completion(completion::Args),
    #[command(before_help = guide::EXAMPLES)]
    Guide(guide::Args),
    #[command(before_help = version::EXAMPLES)]
    Version(version::Args),
    #[command(hide = true, before_help = json::EXAMPLES)]
    Json(json::Args),
    #[command(flatten)]
    Legacy(legacy::Command),
}

/// Owner signing flags, shared by every owner verb.
#[derive(Args, Debug, Clone, Default)]
#[command(next_help_heading = "Signing")]
pub(crate) struct SigningArgs {
    /// Print the owner signing link on stderr (the default)
    #[arg(long, conflicts_with_all = ["sign_with_key", "build_only"])]
    pub sign_with_link: bool,
    /// Sign with a local key (testnet and development)
    #[arg(long, value_name = "KEY", conflicts_with = "build_only")]
    pub sign_with_key: Option<String>,
    /// Write the unsigned transaction to stdout
    #[arg(long)]
    pub build_only: bool,
    /// Wait up to DUR for the owner to sign
    #[arg(long, value_name = "DUR", value_parser = parse_wait)]
    pub wait: Option<Duration>,
}

impl SigningArgs {
    pub(crate) fn resolve(&self, cx: &mut Ctx, rail: &str) -> Result<Signing> {
        let with = if let Some(name) = &self.sign_with_key {
            let key = cx.key(name)?;
            key.check_rail(rail)?;
            SignWith::Key(key)
        } else if self.build_only {
            SignWith::BuildOnly
        } else {
            SignWith::Link
        };
        Ok(Signing {
            with,
            wait: self.wait,
            request_id: None,
            additional: false,
        })
    }
}

/// Parses `60s`, `5m`, `1h30m`, `500ms` or `0`, at most 10 minutes.
pub(crate) fn parse_wait(s: &str) -> std::result::Result<Duration, String> {
    let invalid = || format!("{s:?} is not a duration such as 30s, 5m or 0");
    if s == "0" {
        return Ok(Duration::ZERO);
    }
    let re = regex::Regex::new(r"([0-9]+(?:\.[0-9]*)?|\.[0-9]+)(ms|s|m|h)").unwrap();
    let mut consumed = 0;
    let mut nanos = 0f64;
    for c in re.captures_iter(s) {
        let whole = c.get(0).unwrap();
        if whole.start() != consumed {
            return Err(invalid());
        }
        consumed = whole.end();
        let n = c[1].parse::<f64>().map_err(|_| invalid())?;
        nanos += n * match &c[2] {
            "ms" => 1e6,
            "s" => 1e9,
            "m" => 60e9,
            _ => 3600e9,
        };
    }
    if consumed != s.len() || consumed == 0 {
        return Err(invalid());
    }
    if nanos > 600e9 {
        return Err("the wait must be at most 10m".into());
    }
    Ok(Duration::from_nanos(nanos as u64))
}

/// The full command with the shared help layout on every node.
pub(crate) fn command() -> clap::Command {
    fn layout(cmd: clap::Command) -> clap::Command {
        let groups = cmd.get_subcommands().count() > 0;
        let template = match cmd.get_before_help() {
            Some(examples) => examples_template(&examples.to_string()),
            None => TEMPLATE.to_owned(),
        };
        let cmd = cmd.help_template(template).mut_subcommands(layout);
        // A bare group prints its help and changes nothing.
        if groups && cmd.get_name() != "params" && cmd.get_name() != "allowit" {
            cmd.subcommand_required(true).arg_required_else_help(true)
        } else {
            cmd
        }
    }
    layout(Cli::command())
}

pub(crate) enum Parsed {
    Run(Box<Cli>),
    /// Help or version: print to stdout and exit 0.
    Print(String),
    Error(clap::Error),
}

pub(crate) fn parse(argv: &[String], color: bool) -> Parsed {
    let mut cmd = command().color(if color {
        clap::ColorChoice::Auto
    } else {
        clap::ColorChoice::Never
    });
    match cmd.try_get_matches_from_mut(argv) {
        Ok(mut matches) => {
            let from_flag =
                |id: &str| matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine);
            let (rail_flag, wallet_flag) = (from_flag("rail"), from_flag("wallet"));
            match Cli::from_arg_matches_mut(&mut matches) {
                Ok(mut cli) => {
                    cli.global.rail_flag = rail_flag;
                    cli.global.wallet_flag = wallet_flag;
                    match cli.command {
                        None => Parsed::Print(cmd.render_help().to_string()),
                        Some(_) => Parsed::Run(Box::new(cli)),
                    }
                }
                Err(e) => Parsed::Error(e),
            }
        }
        Err(e) => match e.kind() {
            clap::error::ErrorKind::DisplayHelp
            | clap::error::ErrorKind::DisplayVersion
            | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
                Parsed::Print(e.render().to_string())
            }
            // A bare group prints its help and changes nothing.
            clap::error::ErrorKind::MissingSubcommand => {
                let mut root = command().color(if color {
                    clap::ColorChoice::Auto
                } else {
                    clap::ColorChoice::Never
                });
                root.build();
                Parsed::Print(group(&mut root, argv).render_help().to_string())
            }
            _ => Parsed::Error(e),
        },
    }
}

/// Value flags of the root command, whose values are not command names.
const VALUE_FLAGS: &[&str] = &[
    "--rail",
    "-n",
    "--network",
    "--wallet",
    "--output",
    "--config-dir",
    "--rpc-url",
];

/// The deepest command that the words of `argv` name.
fn group<'a>(root: &'a mut clap::Command, argv: &[String]) -> &'a mut clap::Command {
    let mut current = root;
    let mut skip = false;
    for token in argv.iter().skip(1) {
        if skip {
            skip = false;
            continue;
        }
        if token.starts_with('-') {
            skip = VALUE_FLAGS.contains(&token.as_str());
            continue;
        }
        let name = current
            .get_subcommands()
            .find(|c| c.get_name() == token || c.get_all_aliases().any(|a| a == token))
            .map(|c| c.get_name().to_owned());
        match name {
            Some(name) => current = current.find_subcommand_mut(name).expect("subcommand"),
            None => break,
        }
    }
    current
}

/// The visible subcommands at the point where an unknown command appeared.
pub(crate) fn available_commands(argv: &[String]) -> Vec<String> {
    let mut root = command();
    let current = group(&mut root, argv);
    current
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| c.get_name().to_owned())
        .collect()
}

/// Every command path, hidden aliases included, in tree order. The help
/// snapshot tests run `--help` on each one.
pub(crate) fn paths() -> Vec<Vec<String>> {
    fn walk(cmd: &clap::Command, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        out.push(prefix.clone());
        for sub in cmd.get_subcommands() {
            prefix.push(sub.get_name().to_owned());
            walk(sub, prefix, out);
            prefix.pop();
        }
    }
    let mut out = vec![];
    walk(&command(), &mut vec![], &mut out);
    out
}

pub(crate) fn dispatch(command: Command, cx: &mut Ctx) -> Result<i32> {
    match command {
        Command::Setup(a) => setup::run(a, cx),
        Command::Whoami(a) => whoami::run(a, cx),
        Command::Keys(c) => keys::run(c, cx),
        Command::Rail(c) => rail::run(c, cx),
        Command::Config(c) => config::run(c, cx),
        Command::Wallet(c) => wallet::run(c, cx),
        Command::Policy(c) => policy::run(c, cx),
        Command::Exec(a) => exec::run(a, cx),
        Command::Eval(a) => eval::run(a, cx),
        Command::Status(a) => status::run(a, cx),
        Command::History(a) => history::run(a, cx),
        Command::Tx(c) => tx::run(c, cx),
        Command::Skills(c) => skills::run(c, cx),
        Command::Call(a) => call::run(a, cx),
        Command::Curl(a) => curl::run(a, cx),
        Command::Http(a) => http::run(a, cx),
        Command::Mcp(a) => mcp::run(a, cx),
        Command::Claude(a) => claude::run(a, cx),
        Command::Codex(a) => codex::run(a, cx),
        Command::Lsp(a) => lsp::run(a, cx),
        Command::Completion(a) => completion::run(a, cx),
        Command::Guide(a) => guide::run(a, cx),
        Command::Version(a) => version::run(a, cx),
        Command::Json(a) => json::run(a, cx),
        Command::Legacy(c) => legacy::run(c, cx),
    }
}

/// True for the hidden rail commands of earlier releases: the `tempo` group
/// and the native Solana `policy` verbs. They select a rail only explicitly.
pub(crate) fn is_legacy(command: &Command) -> bool {
    matches!(command, Command::Legacy(legacy::Command::Tempo { .. }))
        || matches!(command, Command::Policy(p) if policy::is_legacy(p))
}

/// True for commands that read no rail, wallet or credential.
pub(crate) fn is_local(command: &Command) -> bool {
    matches!(
        command,
        Command::Lsp(_)
            | Command::Completion(_)
            | Command::Guide(_)
            | Command::Version(_)
            | Command::Json(_)
    ) || matches!(command, Command::Policy(p) if policy::is_local(p))
        || matches!(command, Command::Legacy(l) if legacy::is_local(l))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_command_tree_is_consistent() {
        command().debug_assert();
    }
    #[test]
    fn examples_never_wrap_at_any_width() {
        fn walk(cmd: &mut clap::Command, path: &str) {
            if let Some(examples) = cmd.get_before_help().map(|e| e.to_string()) {
                let help = cmd.clone().term_width(40).render_help().to_string();
                assert!(
                    help.contains(&format!("\n{examples}\n")),
                    "{path}: the examples changed in\n{help}"
                );
            }
            for sub in cmd.get_subcommands_mut() {
                let name = format!("{path} {}", sub.get_name());
                walk(sub, &name);
            }
        }
        let mut root = command();
        root.build();
        walk(&mut root, "allowit");
    }
    #[test]
    fn no_local_argument_reuses_a_global_argument_id() {
        let root = command();
        let globals: Vec<String> = root
            .get_arguments()
            .filter(|a| a.is_global_set())
            .map(|a| a.get_id().to_string())
            .collect();
        fn walk(cmd: &clap::Command, globals: &[String], path: &str) {
            for arg in cmd.get_arguments().filter(|a| !a.is_global_set()) {
                let id = arg.get_id().to_string();
                assert!(
                    !globals.contains(&id),
                    "{path}: argument {id} shadows a global flag"
                );
            }
            for sub in cmd.get_subcommands() {
                walk(sub, globals, &format!("{path} {}", sub.get_name()));
            }
        }
        for sub in root.get_subcommands() {
            walk(sub, &globals, sub.get_name());
        }
    }
    #[test]
    fn every_command_parses_with_global_flags_in_any_position() {
        let argv = |s: &str| {
            std::iter::once("allowit")
                .chain(s.split(' '))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        for line in [
            "policy new prompt -o policy.json --json",
            "--output json policy build policy.rs -o out.rs",
            "rail show solana:devnet --rail stellar:testnet",
            "rail use stellar:testnet --json",
            "exec --op transfer --to G --amount 1 --wallet C --rail stellar:testnet",
            "--json status pay-0001 --wait 5s",
        ] {
            assert!(
                matches!(parse(&argv(line), false), Parsed::Run(_)),
                "{line}"
            );
        }
        let Parsed::Run(cli) = parse(
            &argv("rail show solana:devnet --rail stellar:testnet"),
            false,
        ) else {
            unreachable!()
        };
        assert_eq!(cli.global.rail.as_deref(), Some("stellar:testnet"));
        assert!(cli.global.rail_flag);
        assert!(!cli.global.wallet_flag);
    }
    /// Every command name in the tree.
    fn command_names() -> Vec<String> {
        fn walk(cmd: &clap::Command, out: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                out.push(sub.get_name().to_owned());
                walk(sub, out);
            }
        }
        let mut names = vec![];
        walk(&command(), &mut names);
        names
    }

    /// The words of one example line: `allowit ...`, after a leading
    /// `cat FILE |` and before a later `|` or `>`. Synopsis lines (with
    /// `[`, `(`, `...`, ` = ` or `| COMMAND`) are not examples.
    fn example_words(line: &str) -> Option<Vec<String>> {
        let mut line = line.trim();
        if line.starts_with("cat ")
            && let Some(at) = line.find("| allowit ")
        {
            line = &line[at + 2..];
        }
        let line = line.strip_prefix("allowit ")?;
        let mut words = vec!["allowit".to_owned()];
        let (mut word, mut quote, mut any) = (String::new(), None, false);
        for c in line.chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), c) => word.push(c),
                (None, '\'' | '"') => {
                    quote = Some(c);
                    any = true;
                }
                (None, c) if c.is_whitespace() => {
                    if any || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                (None, c) => word.push(c),
            }
        }
        if any || !word.is_empty() {
            words.push(word);
        }
        let names = command_names();
        let alternatives = words
            .windows(2)
            .any(|w| w[0] == "|" && names.contains(&w[1]));
        if alternatives {
            return None;
        }
        if let Some(at) = words.iter().position(|w| w == "|" || w == ">") {
            words.truncate(at);
        }
        let synopsis = words.iter().take_while(|w| *w != "--").any(|w| {
            w == "=" || w.contains("...") || (w.contains(['[', '|', '(']) && !w.starts_with('{'))
        });
        (!synopsis).then_some(words)
    }

    /// A placeholder such as `ID`, `SERVICE_ID` or `GDEST…`.
    fn placeholder(value: &str) -> bool {
        value.contains('…')
            || (value.starts_with(|c: char| c.is_ascii_uppercase())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
    }

    /// What one example needs from a rail: a verb that `rail show` lists,
    /// the paid-service gateway, or nothing (a local command).
    enum Needs {
        Local,
        Verb(&'static str),
        Gateway(&'static str),
    }

    /// The commands that take the owner signing flags.
    const OWNER_VERBS: &[&str] = &[
        "wallet create",
        "wallet fund",
        "wallet withdraw",
        "wallet update",
        "wallet pause",
        "wallet resume",
        "wallet close",
        "policy deploy",
        "policy params set",
    ];

    /// The paid-service commands, which run through the engine gateway.
    const GATEWAY: &[&str] = &["skills", "curl", "http", "mcp", "claude", "codex"];

    /// What COMMAND (the command words) needs; FUND is true when the words
    /// include `--fund`.
    fn needs(command: &[&str], fund: bool) -> Needs {
        use crate::vendors::VERBS;
        let verb = |name: String| VERBS.iter().copied().find(|v| *v == name);
        match command {
            ["policy", "params", "set", ..] => Needs::Verb("policy params set"),
            ["policy", "params", ..] => Needs::Verb("policy params"),
            ["keys", "import", _, file, ..] if !file.ends_with(".json") => Needs::Local,
            ["keys", "generate", ..] if fund => Needs::Verb("keys fund"),
            ["call", target, ..] if target.starts_with("https://") => Needs::Verb("call"),
            ["call", ..] => Needs::Gateway("call"),
            [a, ..] if GATEWAY.contains(a) => {
                Needs::Gateway(GATEWAY.iter().copied().find(|g| g == a).unwrap_or_default())
            }
            [a, b, ..] if verb(format!("{a} {b}")).is_some() => {
                verb(format!("{a} {b}")).map_or(Needs::Local, Needs::Verb)
            }
            [a, ..] => verb((*a).to_owned()).map_or(Needs::Local, Needs::Verb),
            [] => Needs::Local,
        }
    }

    /// Checks one parsed example: it must parse, and one rail (the example's
    /// `--rail`, or any rail when it names none) must serve its verb, its
    /// signing mode, its `--op`, its parameter keys and its amounts.
    fn check_example(words: &[String]) -> std::result::Result<(), String> {
        use crate::{
            request::request_id_valid,
            routing,
            vendors::{self, SignWith, Signing, asset::Use},
        };
        if let Parsed::Error(e) = parse(words, false) {
            return Err(e.to_string().lines().next().unwrap_or_default().to_owned());
        }
        let words: Vec<&str> = words
            .iter()
            .map(String::as_str)
            .take_while(|w| *w != "--")
            .collect();
        let flag = |name: &str| -> Vec<&str> {
            words
                .windows(2)
                .filter(|w| w[0] == name)
                .map(|w| w[1])
                .collect()
        };
        let has = |name: &str| words.contains(&name);
        let rails: Vec<_> = match flag("--rail").first() {
            Some(r) => vec![routing::info(r).map_err(|e| e.message)?],
            None => routing::RAILS.iter().collect(),
        };
        for id in flag("--request-id") {
            if !placeholder(id) && !request_id_valid(id) {
                return Err(format!("request ID {id:?} breaks the request-ID rule"));
            }
        }
        for value in flag("--wait") {
            parse_wait(value)?;
        }
        // The command words, without the global flags and their values.
        let mut command = vec![];
        let mut skip = false;
        for w in &words[1..] {
            if skip {
                skip = false;
            } else if w.starts_with('-') {
                skip = VALUE_FLAGS.contains(w) || (command.is_empty() && *w == "--wallet");
                if !command.is_empty() {
                    break;
                }
            } else {
                command.push(*w);
            }
        }
        if let ["status", id, ..] = command.as_slice()
            && !placeholder(id)
            && !request_id_valid(id)
        {
            return Err(format!("request ID {id:?} breaks the request-ID rule"));
        }
        let needs = needs(&command, has("--fund"));
        let on = |info: &'static routing::RailInfo| -> std::result::Result<(), String> {
            let rail = vendors::rail(info);
            let caps = rail.caps();
            match needs {
                Needs::Local => {}
                Needs::Verb(verb) if !caps.verbs.contains(&verb) => {
                    return Err(format!("{verb} is not available"));
                }
                Needs::Verb(verb) if OWNER_VERBS.contains(&verb) => {
                    let with = if let Some(name) = flag("--sign-with-key").first() {
                        SignWith::Key(crate::settings::keys::Key {
                            name: (*name).to_owned(),
                            rail: info.id.to_owned(),
                            address: String::new(),
                            public_key: None,
                            curve: caps.curve.to_owned(),
                            secret: true,
                        })
                    } else if has("--build-only") {
                        SignWith::BuildOnly
                    } else {
                        SignWith::Link
                    };
                    let signing = Signing {
                        with,
                        wait: flag("--wait").first().map(|w| parse_wait(w).unwrap()),
                        request_id: None,
                        additional: false,
                    };
                    vendors::require_signing(rail.as_ref(), verb, &signing)
                        .map_err(|e| e.message)?;
                }
                Needs::Verb(_) => {}
                Needs::Gateway(verb) => rail.gateway(verb).map_err(|e| e.message)?,
            }
            let amount = |text: &str, usage: Use| -> std::result::Result<(), String> {
                if placeholder(text) {
                    return Ok(());
                }
                rail.parse_amount(text, usage)
                    .map(|_| ())
                    .map_err(|e| format!("amount {text:?}: {}", e.message))
            };
            match command.as_slice() {
                ["wallet", "fund", value, ..] => amount(value, Use::Spend)?,
                ["wallet", "withdraw", value, ..] => amount(value, Use::Withdraw)?,
                ["policy", "params", "set", pairs @ ..] => {
                    for pair in pairs {
                        if placeholder(pair) {
                            continue;
                        }
                        let param =
                            policy::params_set_parse(rail.as_ref(), pair).map_err(|e| e.message)?;
                        if !caps.param_keys.contains(&param.key.as_str()) {
                            return Err(format!(
                                "policy params set {} is not available",
                                param.key
                            ));
                        }
                    }
                }
                _ => {}
            }
            if let ["exec" | "eval", ..] = command.as_slice() {
                for op in flag("--op") {
                    vendors::require_op(rail.as_ref(), op).map_err(|e| e.message)?;
                }
            }
            for value in [flag("--amount"), flag("--fund")].concat() {
                amount(value, Use::Spend)?;
            }
            for value in [
                flag("--max-amount"),
                flag("--max-payment"),
                flag("--budget"),
            ]
            .concat()
            {
                let decimal = vendors::asset::decimal_units(value, 18).is_ok_and(|u| u > 0);
                if !decimal {
                    amount(value, Use::Spend)?;
                }
            }
            Ok(())
        };
        let mut errors = vec![];
        for info in rails {
            match on(info) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(format!("{}: {e}", info.id)),
            }
        }
        Err(errors.join("; "))
    }

    #[test]
    fn example_checks_catch_bad_values() {
        let words = |s: &str| example_words(s).unwrap();
        for bad in [
            "allowit status pay-001 --wait 60s",
            "allowit exec --op transfer --to GDEST… --amount XLM:5 --request-id pay-001",
            "allowit exec --op transfer --to GDEST… --amount XLM:-5",
            "allowit --rail tempo:testnet exec --op transfer --to 0xR… --amount XLM:5",
            "allowit wallet fund XLM:0",
            "allowit policy params set transaction_cap=XLM:0",
            "allowit status pay-0001 --wait 1d",
            "allowit exec --op transfer --amount XLM:5",
            "allowit keys generate",
            "allowit wallet fund XLM:200 --from owner",
            "allowit wallet pause",
            "allowit --rail solana:devnet wallet pause --sign-with-key owner --wait 60s",
            "allowit --rail solana:devnet wallet pause --build-only",
            "allowit --rail solana:devnet policy params set call_cap=20 --sign-with-key owner",
            "allowit eval --op transfer --to GDEST… --amount XLM:5",
            "allowit tx send signed.json",
            "allowit curl https://api.example/v1/data",
            "allowit call SERVICE_ID",
            "allowit --rail stellar:testnet call https://api.example/v1/data --max-amount 0.05",
            "allowit --rail tempo:testnet keys generate agent --fund",
        ] {
            assert!(check_example(&words(bad)).is_err(), "{bad}");
        }
        assert_eq!(
            words("cat signed.json | allowit tx send -"),
            ["allowit", "tx", "send", "-"]
        );
        assert_eq!(
            words("allowit keys export agent | pass insert -m allowit/agent"),
            ["allowit", "keys", "export", "agent"]
        );
        assert_eq!(
            words("allowit call S --input '{\"city\":\"Paris\"}'"),
            ["allowit", "call", "S", "--input", "{\"city\":\"Paris\"}"]
        );
        for good in [
            "allowit --rail solana:devnet wallet pause --sign-with-key owner",
            "allowit --rail tempo:testnet call https://api.example/v1/data --max-amount 0.05",
            "allowit keys import agent agent.key",
            "allowit --rail stellar:testnet keys generate agent --fund",
        ] {
            assert_eq!(check_example(&words(good)), Ok(()), "{good}");
        }
        assert!(example_words("allowit status ID [--wait DUR]").is_none());
        assert!(example_words("allowit wallet pause | resume | close").is_none());
        assert!(example_words("allowit mcp | claude").is_none());
    }

    /// Every example in help, the guide and the READMEs parses, and its
    /// request IDs, amounts, parameters and waits are valid.
    #[test]
    fn every_example_parses_with_valid_values() {
        fn walk(cmd: &clap::Command, path: String, out: &mut Vec<(String, String)>) {
            if let Some(help) = cmd.get_before_help() {
                for line in help.to_string().lines() {
                    out.push((path.clone(), line.to_owned()));
                }
            }
            for sub in cmd.get_subcommands() {
                walk(sub, format!("{path} {}", sub.get_name()), out);
            }
        }
        let mut lines = vec![];
        walk(&command(), "allowit".into(), &mut lines);
        for (doc, text) in [
            ("guide.md", include_str!("guide/guide.md")),
            ("cli/README.md", include_str!("../../README.md")),
            ("README.md", include_str!("../../../README.md")),
        ] {
            lines.extend(text.lines().map(|l| (doc.to_owned(), l.to_owned())));
        }
        let mut checked = 0;
        let mut failures = vec![];
        for (source, line) in &lines {
            let Some(words) = example_words(line) else {
                continue;
            };
            checked += 1;
            if let Err(e) = check_example(&words) {
                failures.push(format!("{source}: {}: {e}", line.trim()));
            }
        }
        assert!(checked > 120, "{checked} examples");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A node that some rail serves lists examples; a node that no rail
    /// serves lists none, since none of them could run.
    #[test]
    fn served_nodes_and_only_they_have_examples() {
        fn served(cmd: &clap::Command, path: &[&str]) -> bool {
            let children: Vec<_> = cmd
                .get_subcommands()
                .filter(|c| c.get_name() != "help")
                .collect();
            if !children.is_empty() {
                return children
                    .iter()
                    .any(|c| served(c, &[path, &[c.get_name()]].concat()));
            }
            let needs = if path == ["call"] {
                Needs::Verb("call")
            } else {
                needs(path, false)
            };
            crate::routing::RAILS.iter().any(|info| {
                let rail = crate::vendors::rail(info);
                match needs {
                    Needs::Local => true,
                    Needs::Verb(verb) => rail.caps().verbs.contains(&verb),
                    Needs::Gateway(verb) => rail.gateway(verb).is_ok(),
                }
            })
        }
        fn walk(cmd: &clap::Command, path: Vec<&str>, failures: &mut Vec<String>) {
            let examples = cmd
                .get_before_help()
                .is_some_and(|h| h.to_string().contains("Examples:\n  "));
            if served(cmd, &path) != examples {
                failures.push(format!(
                    "allowit {}: served {}, examples {examples}",
                    path.join(" "),
                    !examples
                ));
            }
            for sub in cmd.get_subcommands().filter(|c| c.get_name() != "help") {
                let mut next = path.clone();
                next.push(sub.get_name());
                walk(sub, next, failures);
            }
        }
        let root = command();
        let mut failures = vec![];
        walk(&root, vec![], &mut failures);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn waits_parse_and_are_bounded() {
        assert_eq!(parse_wait("60s").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_wait("1m30s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_wait("0").unwrap(), Duration::ZERO);
        assert_eq!(parse_wait("500ms").unwrap(), Duration::from_millis(500));
        for bad in ["", "10", "-1s", "11m", "1d", "5 m"] {
            assert!(parse_wait(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn unknown_commands_list_the_commands_at_that_level() {
        let argv = |s: &[&str]| s.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let top = available_commands(&argv(&["allowit", "nope"]));
        assert!(top.contains(&"keys".to_owned()) && top.contains(&"exec".to_owned()));
        assert!(!top.contains(&"json".to_owned()));
        let keys = available_commands(&argv(&["allowit", "--rail", "x", "keys", "nope"]));
        assert!(keys.contains(&"generate".to_owned()));
    }
}
