//! Tempo adapter over `allowit-tempo`, testnet only.
//!
//! - `keys import`: the owner-exported executor bundle.
//! - `exec --op transfer`: one vault payment signed by the agent key that
//!   `setup --agent` chose; `--build-only` asks the engine for the
//!   authority-signed wallet plan and prints it as JSON, unsigned by the
//!   agent and unsubmitted.
//! - `status`, `wallet show`, `policy show`: the bundle and its engine.
//! - `call URL`: one MPP-paid HTTPS GET. `--max-amount` takes a decimal of
//!   the vault token or the vault token's `CODE:DECIMAL`.
//!
//! Every amount must name the bundle's installed token; another token exits 4.
//! The bundle is `ALLOWIT_TEMPO_BUNDLE` (default `.allowit-tempo/executor.json`)
//! and must belong to the selected rail. The crate always prints JSON here;
//! the adapter renders it through the output module and passes the agent key,
//! `--rpc-url` (or the rail's `rpc_url`) and `--wait` as the crate's
//! settings. The hidden `tempo` group keeps the commands of earlier releases.
pub(crate) mod formats;

use super::{
    Binding, CallRequest, Caps, ExecRequest, Mode, Rail,
    asset::{Amount, Asset, Token},
    require_op,
};
use crate::{
    config::{self, env_value},
    ctx::Ctx,
    error::{Error, Result},
    routing::{RailInfo, normalize, selected},
    settings::keys,
};
use serde_json::Value;
use std::{path::Path, time::Duration};

pub(crate) struct Tempo(pub &'static RailInfo);

const TOKENS: &[Token] = &[
    Token {
        code: "AlphaUSD",
        decimals: 6,
        native: false,
        issuer: Some("0x20c0000000000000000000000000000000000001"),
    },
    Token {
        code: "pathUSD",
        decimals: 6,
        native: false,
        issuer: Some("0x20c0000000000000000000000000000000000000"),
    },
];
const DECIMALS: u8 = 6;

fn bundle_path() -> String {
    match env_value("ALLOWIT_TEMPO_BUNDLE") {
        v if v.is_empty() => ".allowit-tempo/executor.json".into(),
        v => v,
    }
}

/// The crate's finality wait bound, in seconds.
const MAX_WAIT_SECONDS: u64 = 600;

/// The saved stderr while [`without_crate_progress`] sends it to /dev/null,
/// or -1.
#[cfg(unix)]
static SAVED_STDERR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Points file descriptor 2 back at the original stderr, so that a panic
/// inside the crate still reports. The saved descriptor stays open for the
/// pending [`without_crate_progress`] to close.
#[cfg(unix)]
pub(crate) fn restore_stderr() {
    let saved = SAVED_STDERR.load(std::sync::atomic::Ordering::SeqCst);
    if saved >= 0 {
        // SAFETY: the descriptor stays open while it is published.
        unsafe {
            libc::dup2(saved, 2);
        }
    }
}
#[cfg(not(unix))]
pub(crate) fn restore_stderr() {}

/// Runs `f` with stderr (file descriptor 2) sent to /dev/null. The crate
/// writes its own progress lines straight to the process stderr; the adapter
/// reports through the output module instead.
#[cfg(unix)]
fn without_crate_progress<T>(f: impl FnOnce() -> T) -> T {
    use std::{os::fd::AsRawFd, sync::atomic::Ordering};
    struct Restore(i32);
    impl Drop for Restore {
        fn drop(&mut self) {
            SAVED_STDERR.store(-1, Ordering::SeqCst);
            // SAFETY: the descriptor came from dup(2) and is closed once.
            unsafe {
                libc::dup2(self.0, 2);
                libc::close(self.0);
            }
        }
    }
    let Ok(sink) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return f();
    };
    // SAFETY: plain descriptor calls; a failed dup leaves stderr unchanged.
    let saved = unsafe { libc::dup(2) };
    if saved < 0 {
        return f();
    }
    let _restore = Restore(saved);
    SAVED_STDERR.store(saved, Ordering::SeqCst);
    // SAFETY: both descriptors are open for the duration of the call.
    if unsafe { libc::dup2(sink.as_raw_fd(), 2) } < 0 {
        return f();
    }
    f()
}
#[cfg(not(unix))]
fn without_crate_progress<T>(f: impl FnOnce() -> T) -> T {
    f()
}

impl Tempo {
    fn testnet_only(&self, verb: &str) -> Result<()> {
        if self.0.testnet {
            Ok(())
        } else {
            Err(self.na(verb))
        }
    }
    /// The public policy of the bundle, checked against the rail and wallet.
    fn describe(&self, wallet: Option<&str>) -> Result<serde_json::Value> {
        let policy = allowit_tempo::describe(Path::new(&bundle_path()))?;
        let network = policy["network"]
            .as_str()
            .ok_or_else(|| Error::config("the Tempo bundle names no network"))?;
        if normalize(network).ok() != Some(self.0.id) {
            return Err(Error::config(format!(
                "the Tempo bundle belongs to {network}, not {}",
                self.0.id
            )));
        }
        if let Some(w) = wallet
            && !policy["vault"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(w))
        {
            return Err(Error::config(format!(
                "the Tempo bundle is for vault {}, not {w}",
                policy["vault"].as_str().unwrap_or("?")
            )));
        }
        Ok(policy)
    }
    /// The public policy of the bundle, checked against the selected wallet.
    fn describe_selected(&self, cx: &mut Ctx) -> Result<serde_json::Value> {
        let wallet = cx.wallet(self.0)?;
        self.describe(wallet.as_deref())
    }
    /// A vault amount as a decimal. The amount must name the token that the
    /// bundle's vault holds.
    fn decimal(&self, amount: &Amount, policy: &serde_json::Value) -> Result<String> {
        let installed = policy["token"].as_str().unwrap_or_default();
        let ok = match &amount.asset {
            Asset::Installed => true,
            Asset::Token {
                issuer: Some(token),
                ..
            } => !installed.is_empty() && token.eq_ignore_ascii_case(installed),
            _ => false,
        };
        if !ok {
            return Err(Error {
                code: 4,
                message: format!(
                    "the vault holds only its token {installed}; {} names another asset",
                    amount.text
                ),
                status: None,
                config: false,
            });
        }
        let units = amount
            .units()
            .ok_or_else(|| Error::usage("an exact amount is required"))?;
        Ok(super::asset::format_units(units, DECIMALS))
    }
    /// A paid-call maximum: a decimal of the vault token, or an amount.
    fn max_amount(&self, text: &str) -> Result<Amount> {
        if text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
            let units = super::asset::decimal_units(text, DECIMALS)?;
            if units == 0 {
                return Err(Error::usage("--max-amount must be positive"));
            }
            return Ok(Amount {
                asset: Asset::Installed,
                quantity: super::asset::Quantity::Units(units),
                decimals: Some(DECIMALS),
                text: text.into(),
            });
        }
        self.parse_amount(text, super::asset::Use::Spend)
    }
    /// The crate settings of one command: the RPC override, the finality
    /// wait when given, and the agent key for a command that signs.
    fn settings(
        &self,
        cx: &mut Ctx,
        signs: bool,
        wait: Option<Duration>,
    ) -> Result<Vec<(&'static str, String)>> {
        let mut settings = vec![];
        if let Some(url) = cx.rpc_url(self.0)? {
            settings.push(("ALLOWIT_TEMPO_RPC_URL", url));
        }
        if let Some(wait) = wait {
            let seconds = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
            if seconds > MAX_WAIT_SECONDS {
                return Err(Error::usage("--wait on Tempo is at most 10m"));
            }
            settings.push(("ALLOWIT_TEMPO_WAIT_SECONDS", seconds.to_string()));
        }
        if !signs {
            return Ok(settings);
        }
        // The agent key that setup chose signs; without one, the key file
        // of earlier releases.
        if cx.settings()?.rail(self.0.id).agent.is_some()
            || env_value("ALLOWIT_TEMPO_EXECUTOR_KEY_FILE").is_empty()
        {
            let agent = cx.agent(self.0)?;
            let signer = cx.signer(&agent)?;
            settings.push((
                "ALLOWIT_TEMPO_EXECUTOR_KEY_FILE",
                signer.to_string_lossy().into_owned(),
            ));
        }
        Ok(settings)
    }
    /// Runs one crate command with `--json` and returns its exit code and
    /// JSON result.
    fn run(
        &self,
        cx: &mut Ctx,
        mut args: Vec<String>,
        id: Option<&str>,
        settings: &[(&'static str, String)],
    ) -> Result<(i32, Option<Value>)> {
        args.push("--json".into());
        let (mut stdout, mut stderr) = (String::new(), String::new());
        crate::flush_stderr(&mut cx.out.stderr);
        let code = config::with_settings(settings, || {
            without_crate_progress(|| {
                allowit_tempo::run_with_request_id(&args, id, &mut stdout, &mut stderr)
            })
        })?;
        Ok((code, serde_json::from_str(&stdout).ok()))
    }
    /// Renders a crate result through the output module.
    fn show(cx: &mut Ctx, code: i32, value: Option<Value>) -> Result<i32> {
        match value {
            Some(v) => {
                cx.out.value(&v);
                Ok(code)
            }
            None if code == 0 => Err(Error::uncertain("allowit-tempo returned no JSON result")),
            // A failure keeps its exit code and still reports on stderr.
            None => Err(Error {
                code,
                message: format!("allowit-tempo exited {code} without a JSON result"),
                status: None,
                config: false,
            }),
        }
    }
    /// Renders an `execute` result for `exec --build-only`. The crate exits
    /// 10 when the engine returns the authority-signed wallet plan; that plan
    /// is what build-only asks for, so it prints as JSON and exits 0. A
    /// denial, an owner-input request or a settled request keeps its code.
    fn show_plan(cx: &mut Ctx, code: i32, value: Option<Value>) -> Result<i32> {
        match (code, value) {
            (10, Some(plan)) => {
                cx.out.json_value(&plan);
                Ok(0)
            }
            (code, value) => Self::show(cx, code, value),
        }
    }
    /// The public policy with its canonical rail ID.
    fn view(&self, mut policy: Value) -> Value {
        policy["network"] = Value::String(self.0.id.into());
        policy
    }
}

impl Rail for Tempo {
    fn info(&self) -> &'static RailInfo {
        self.0
    }
    fn tokens(&self) -> &'static [Token] {
        TOKENS
    }
    fn caps(&self) -> Caps {
        Caps {
            curve: "secp256k1",
            address_is_key: false,
            // Tempo testnet has no USDC; AlphaUSD is its TIP-20 USD token.
            service_asset: if self.0.testnet { "AlphaUSD" } else { "USDC" },
            ops: &["transfer"],
            param_keys: &[],
            signing: &[],
            verbs: if self.0.testnet {
                &[
                    "keys import",
                    "wallet show",
                    "policy show",
                    "exec",
                    "status",
                    "call",
                ]
            } else {
                &[]
            },
        }
    }
    fn keys_import(
        &self,
        cx: &mut Ctx,
        name: &str,
        file: &Path,
        bundle: &serde_json::Value,
    ) -> Result<i32> {
        self.testnet_only("keys import")?;
        let network = bundle["policy"]["network"].as_str().unwrap_or_default();
        if normalize(network).ok() != Some(self.0.id) {
            return Err(Error::config(format!(
                "the Tempo bundle belongs to {network}, not {}",
                self.0.id
            )));
        }
        let executor = bundle["policy"]["executor"]
            .as_str()
            .ok_or_else(|| Error::usage("the Tempo bundle names no executor"))?
            .to_owned();
        let dir = cx.dir()?;
        keys::valid_name(name)?;
        if keys::exists(&dir, name) {
            return Err(Error::config(format!("key {name} already exists")));
        }
        let policy = bundle["policy"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let args = vec!["import".into(), file.to_string_lossy().into_owned()];
        let (code, _) = self.run(cx, args, None, &[])?;
        if code != 0 {
            return Ok(code);
        }
        let key = keys::add(&dir, name, self.0.id, &executor)?;
        let mut view = key.view();
        view["policy"] = Value::String(policy);
        cx.out.value(&view);
        Ok(0)
    }
    fn wallet_show(&self, cx: &mut Ctx, wallet: Option<&str>) -> Result<i32> {
        self.testnet_only("wallet show")?;
        let selected = match wallet {
            Some(w) => Some(w.to_owned()),
            None => cx.wallet(self.0)?,
        };
        let policy = self.describe(selected.as_deref())?;
        cx.out.value(&self.view(policy));
        Ok(0)
    }
    fn legacy(&self, cx: &mut Ctx, verb: &str, args: &[String]) -> Result<i32> {
        if verb != "tempo" {
            return Err(self.na(verb));
        }
        let out = &mut cx.out;
        legacy(args, &mut out.stdout, &mut out.stderr)
    }
    fn policy_show(&self, cx: &mut Ctx, source: bool) -> Result<i32> {
        self.testnet_only("policy show")?;
        if source {
            return Err(self.na("policy show --source"));
        }
        let policy = self.describe_selected(cx)?;
        cx.out.value(&self.view(policy));
        Ok(0)
    }
    fn exec_binding(&self, _: &mut Ctx, wallet: Option<&str>) -> Result<Binding> {
        let policy = self.describe(wallet)?;
        let text = |key: &str| {
            policy[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::config(format!("the Tempo bundle names no {key}")))
        };
        Ok(Binding {
            wallet: Some(text("vault")?),
            policy: Some(format!("{}@{}", text("id")?, text("policyDigest")?)),
        })
    }
    fn exec(&self, cx: &mut Ctx, r: &ExecRequest) -> Result<i32> {
        self.testnet_only("exec")?;
        require_op(self, &r.op)?;
        if r.mode == Mode::Eval {
            return Err(self.na("eval"));
        }
        if r.token_out.is_some() || r.args.is_some() {
            return Err(Error::usage(
                "--token-out and --args apply to swap and custom operations, not transfer",
            ));
        }
        super::address::check_recipient(self.0.family, &r.to)?;
        let policy = self.describe(r.wallet.as_deref())?;
        let amount = self.decimal(&r.amount, &policy)?;
        // execute builds the wallet plan only; pay signs and waits.
        let (verb, settings) = if r.build_only {
            ("execute", self.settings(cx, false, None)?)
        } else {
            ("pay", self.settings(cx, true, Some(r.wait))?)
        };
        let args = vec![verb.into(), r.to.clone(), amount];
        let (code, value) = self.run(cx, args, Some(&r.request_id), &settings)?;
        if r.build_only {
            return Self::show_plan(cx, code, value);
        }
        Self::show(cx, code, value)
    }
    fn status(&self, cx: &mut Ctx, id: &str, wallet: Option<&str>, wait: Duration) -> Result<i32> {
        self.testnet_only("status")?;
        self.describe(wallet)?;
        let settings = self.settings(cx, false, None)?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            let args = vec!["status".into(), id.into()];
            let (code, value) = self.run(cx, args, None, &settings)?;
            // Submitted (12) and unknown (5) states may still settle.
            if !matches!(code, 5 | 12) || std::time::Instant::now() >= deadline {
                return Self::show(cx, code, value);
            }
            std::thread::sleep(Duration::from_secs(2).min(wait));
        }
    }
    fn call(&self, cx: &mut Ctx, c: &CallRequest) -> Result<Option<i32>> {
        if !c.target.starts_with("https://") || !self.0.testnet {
            return Ok(None);
        }
        if c.input.is_some() {
            return Err(Error::usage("an MPP call is one GET without --input"));
        }
        let max = c
            .max_amount
            .as_deref()
            .ok_or_else(|| Error::usage("an MPP call needs --max-amount"))?;
        let max = self.max_amount(max)?;
        let policy = self.describe_selected(cx)?;
        let units = self.decimal(&max, &policy)?;
        let units = super::asset::decimal_units(&units, DECIMALS)?;
        let exact = format!("{}.{:06}", units / 1_000_000, units % 1_000_000);
        let id = match &c.request_id {
            Some(id) => id.clone(),
            None => {
                let id = crate::settings::journal::new_id()?;
                cx.out.generated_id(&id);
                id
            }
        };
        let settings = self.settings(cx, true, None)?;
        let args = vec![
            "call".into(),
            c.target.clone(),
            "--id".into(),
            id,
            "--max-amount".into(),
            exact,
        ];
        let (code, value) = self.run(cx, args, None, &settings)?;
        Self::show(cx, code, value).map(Some)
    }
}

/// The hidden `tempo` group of earlier releases. With a selected rail, the
/// bundle must belong to it.
pub(crate) fn legacy(args: &[String], stdout: &mut String, stderr: &mut String) -> Result<i32> {
    // Help anywhere in the words prints the group usage.
    if args.first().is_some_and(|s| s == "help")
        || args.iter().any(|s| matches!(s.as_str(), "-h" | "--help"))
    {
        return allowit_tempo::run(&["help".to_owned()], stdout, stderr);
    }
    if let Some(selected) = selected() {
        let path = if args.first().is_some_and(|s| s == "import") {
            args.iter()
                .skip(1)
                .find(|a| *a != "--json")
                .cloned()
                .ok_or_else(|| Error::usage("tempo import needs a bundle path"))?
        } else {
            bundle_path()
        };
        let policy = allowit_tempo::describe(Path::new(&path))?;
        let network = policy["network"]
            .as_str()
            .ok_or_else(|| Error::config("the Tempo bundle names no network"))?;
        if normalize(network).ok() != Some(selected.as_str()) {
            return Err(Error::config(
                "the selected rail differs from the Tempo wallet deployment",
            ));
        }
    }
    allowit_tempo::run(args, stdout, stderr)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panic report while the crate's progress is silenced reaches the
    /// original stderr.
    #[cfg(unix)]
    #[test]
    fn a_panic_report_reaches_the_original_stderr() {
        let identity = |fd: i32| {
            // SAFETY: fstat writes into a zeroed stat buffer.
            unsafe {
                let mut st = std::mem::zeroed::<libc::stat>();
                assert_eq!(libc::fstat(fd, &mut st), 0);
                (st.st_dev, st.st_ino)
            }
        };
        let before = identity(2);
        without_crate_progress(|| {
            restore_stderr();
            assert_eq!(identity(2), before);
        });
        assert_eq!(identity(2), before);
        assert_eq!(SAVED_STDERR.load(std::sync::atomic::Ordering::SeqCst), -1);
    }
    #[test]
    fn a_build_only_plan_prints_json_and_exits_0() {
        let mut cx = super::super::tests::ctx();
        let plan = serde_json::json!({"decision": "allow", "requestId": "plan-0001"});
        assert_eq!(
            Tempo::show_plan(&mut cx, 10, Some(plan.clone())).unwrap(),
            0
        );
        let printed: Value = serde_json::from_str(&cx.out.stdout).unwrap();
        assert_eq!(printed, plan);
        let mut cx = super::super::tests::ctx();
        let denied = serde_json::json!({"decision": "deny"});
        assert_eq!(Tempo::show_plan(&mut cx, 20, Some(denied)).unwrap(), 20);
        assert_eq!(cx.out.stdout, "decision  deny\n");
        let mut cx = super::super::tests::ctx();
        assert_eq!(Tempo::show_plan(&mut cx, 10, None).unwrap_err().code, 10);
    }
    #[test]
    fn tokens_are_tip20_addresses_with_six_decimals() {
        for t in TOKENS {
            super::super::address::check_token(crate::routing::Family::Tempo, t.issuer.unwrap())
                .unwrap();
            assert_eq!(t.decimals, 6);
        }
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let policy = serde_json::json!({"token": "0x20C0000000000000000000000000000000000001"});
        let a = rail
            .parse_amount("AlphaUSD:1.5", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&a, &policy).unwrap(), "1.5");
        let base = rail
            .parse_amount("1500000", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&base, &policy).unwrap(), "1.5");
    }
    #[test]
    fn amounts_of_another_token_exit_4() {
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let policy = serde_json::json!({"token": TOKENS[0].issuer.unwrap()});
        for other in [
            "pathUSD:1",
            "AlphaUSD:0x20c0000000000000000000000000000000000000:1",
            "FOO:0x20c00000000000000000000000000000000000ff:1",
        ] {
            let a = rail
                .parse_amount(other, super::super::asset::Use::Spend)
                .unwrap();
            assert_eq!(rail.decimal(&a, &policy).unwrap_err().code, 4, "{other}");
        }
        let explicit = rail
            .parse_amount(
                "USD:0x20C0000000000000000000000000000000000001:2",
                super::super::asset::Use::Spend,
            )
            .unwrap();
        assert_eq!(rail.decimal(&explicit, &policy).unwrap(), "2");
        let none = serde_json::json!({});
        let base = rail
            .parse_amount("1", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&base, &none).unwrap(), "0.000001");
        let named = rail
            .parse_amount("AlphaUSD:1", super::super::asset::Use::Spend)
            .unwrap();
        assert_eq!(rail.decimal(&named, &none).unwrap_err().code, 4);
    }
    #[test]
    fn paid_call_maximums_are_decimals_of_the_vault_token() {
        let rail = Tempo(crate::routing::info("tempo:testnet").unwrap());
        let policy = serde_json::json!({"token": TOKENS[0].issuer.unwrap()});
        for (text, decimal) in [("0.05", "0.05"), ("1", "1"), ("AlphaUSD:0.1", "0.1")] {
            let max = rail.max_amount(text).unwrap();
            assert_eq!(rail.decimal(&max, &policy).unwrap(), decimal, "{text}");
        }
        for bad in ["0", "0.0000001", ".5", "1."] {
            assert_eq!(rail.max_amount(bad).unwrap_err().code, 2, "{bad}");
        }
        let other = rail.max_amount("pathUSD:0.1").unwrap();
        assert_eq!(rail.decimal(&other, &policy).unwrap_err().code, 4);
    }
}
