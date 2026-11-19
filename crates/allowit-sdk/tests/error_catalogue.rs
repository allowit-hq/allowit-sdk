//! Every typed error code that the SDK names has an entry in the error catalogue. Hosts show
//! the entry's title with the sender's own message and the help page of the code.
//!
//! The test reads the SDK's source, so a code that a new refusal defines is checked without a
//! list to keep: a string constant with a code-like value, and the code argument of a refusal
//! (`refused(status, "code", …)`, `api_code = Some("code".into())`). The unit tests of a file are
//! left out. A code-like constant that is no error code is in `NOT_CODES`.
use allowit_sdk::{custom_build::code, errors, paid_service, transport_failure};
use std::path::Path;

/// Code-like constants of the SDK that are not error codes.
const NOT_CODES: &[(&str, &str)] = &[];

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            let production = text
                .find("#[cfg(test)]\nmod ")
                .map_or(text.as_str(), |end| &text[..end])
                .to_owned();
            out.push((path.display().to_string(), production));
        }
    }
}

/// The string literal at the start of `text`, after any spaces.
fn literal(text: &str) -> Option<&str> {
    let rest = text.trim_start().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

/// True for lowercase words joined by underscores, such as `network_unavailable`.
fn code_like(word: &str) -> bool {
    errors::is_code(word) && word.contains('_') && !word.starts_with('_') && !word.ends_with('_')
}

/// The typed codes that `text` names: a string constant with a code-like value, the code argument
/// of a refusal with or without a status before it, and a code set as `api_code`.
fn codes_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some((head, value)) = line.split_once(": &str =")
            && head.contains("const ")
            && let Some(word) = literal(value)
            && code_like(word)
        {
            out.push(word.to_owned());
        }
        if let Some((_, value)) = line.split_once("api_code = Some(")
            && let Some(word) = literal(value)
            && errors::is_code(word)
        {
            out.push(word.to_owned());
        }
    }
    // A refusal's arguments can span lines: its code is the first string literal, when it comes
    // first or after one argument that is no string.
    for (start, _) in text.match_indices("refused(") {
        let args = &text[start + "refused(".len()..];
        let word = literal(args).or_else(|| {
            let first = args.find(['"', ','])?;
            (args.as_bytes()[first] == b',').then(|| literal(&args[first + 1..]))?
        });
        if let Some(word) = word.filter(|word| errors::is_code(word)) {
            out.push(word.to_owned());
        }
    }
    out
}

#[test]
fn the_scan_reads_every_form_of_a_code() {
    let text = r#"
        pub const NAMED: &str = "named_code";
        Error::refused(409, "single", "text")
        Error::refused(status_of(reply), "after_a_call", "text")
        error.api_code = Some("set_code".into());
        let text = "Not a code";
    "#;
    let mut found = codes_in(text);
    found.sort();
    assert_eq!(found, ["after_a_call", "named_code", "set_code", "single"]);
}

#[test]
fn every_code_that_the_sdk_names_has_a_catalogue_entry() {
    let mut files = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() > 10);
    let found: Vec<(String, String)> = files
        .iter()
        .flat_map(|(file, text)| codes_in(text).into_iter().map(|c| (c, file.clone())))
        .collect();
    // The codes that the SDK is known to name: the scan found them.
    for expected in [
        transport_failure::NETWORK_UNAVAILABLE_NAME,
        paid_service::SELLER_FAILED_NAME,
    ] {
        assert!(
            found.iter().any(|(code, _)| code == expected),
            "{expected} was not found in the source"
        );
    }
    let not_codes: Vec<&str> = NOT_CODES.iter().map(|(word, _)| *word).collect();
    let missing: Vec<String> = found
        .iter()
        .filter(|(code, _)| !not_codes.contains(&code.as_str()) && errors::entry(code).is_none())
        .map(|(code, file)| format!("{code} ({file})"))
        .collect();
    assert!(
        missing.is_empty(),
        "these typed codes have no entry in the error catalogue of allowit-core; add the entry \
         there, or list a word that is no error code in NOT_CODES:\n{}",
        missing.join("\n")
    );
    // The codes that the SDK passes on from Core's custom build.
    for named in [
        code::UNKNOWN_POLICY,
        code::CODE_HASH_MISMATCH,
        code::TOOLCHAIN_MISSING,
        code::BUILD_FAILED,
        code::BUILD_MISSING,
    ] {
        assert!(errors::entry(named).is_some(), "{named} has no entry");
    }
    assert!(errors::entry("no_such_code").is_none());
    assert_eq!(
        errors::help_url(
            "https://app.example",
            transport_failure::NETWORK_UNAVAILABLE_NAME
        )
        .as_deref(),
        Some("https://app.example/help/network_unavailable")
    );
}
