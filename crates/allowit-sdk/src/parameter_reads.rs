//! Deterministic typed parameter reads from portable policy source.
//! This analysis does not compile or admit the policy or grant execution authority.
use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Maximum group nesting accepted by the shared source parser.
pub const MAX_NESTING: usize = 32;
/// Maximum operators and keywords along one statement path.
pub const MAX_PATH_SYNTAX: usize = 160;
// Parsing, visiting and dropping syn trees need the same reserved stack as the source gate.
const PARSE_STACK: usize = 64 << 20;

/// Whether `expr` is `allowit::<NAME>`.
fn allowit_path(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expr else {
        return None;
    };
    let names: Vec<String> = path
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    match names.as_slice() {
        [root, name] if root == "allowit" && path.qself.is_none() => Some(name.clone()),
        _ => None,
    }
}

/// Rejects a source whose syntax tree could nest deeply enough to overflow the parser's stack.
/// The token walk is iterative, and it bounds group depth and the operators and keywords on
/// every statement path before `syn` parses the source recursively.
pub fn check_source_shape(source: &str) -> Result<()> {
    use proc_macro2::{Delimiter, TokenStream, TokenTree};
    let too_deep = || {
        Error::usage(format!(
            "The policy nests too deeply: use at most {MAX_NESTING} levels of brackets and at most {MAX_PATH_SYNTAX} operators and keywords in one statement. Split it into simpler statements."
        ))
    };
    let tokens: TokenStream = source
        .parse()
        .map_err(|error| Error::usage(format!("The policy is not valid Rust: {error}.")))?;
    // Each open group keeps its token iterator, whether it holds statements, and the path
    // count at its start; a statement in a block restarts from the block's count.
    let mut stack = vec![(tokens.into_iter(), true, 0usize)];
    let mut path = 0usize;
    while let Some((iter, statements, base)) = stack.last_mut() {
        let Some(token) = iter.next() else {
            path = *base;
            stack.pop();
            continue;
        };
        let (statements, base) = (*statements, *base);
        match token {
            TokenTree::Group(group) => {
                if stack.len() > MAX_NESTING {
                    return Err(too_deep());
                }
                let block = group.delimiter() == Delimiter::Brace;
                stack.push((group.stream().into_iter(), block, path));
            }
            TokenTree::Punct(p) if p.as_char() == ';' && statements => path = base,
            TokenTree::Punct(p) if !matches!(p.as_char(), ',' | ';' | ':' | '#' | '\'') => {
                path += 1
            }
            TokenTree::Ident(ident)
                if matches!(
                    ident.to_string().as_str(),
                    "if" | "else"
                        | "match"
                        | "while"
                        | "for"
                        | "loop"
                        | "let"
                        | "return"
                        | "break"
                        | "yield"
                        | "move"
                        | "async"
                        | "unsafe"
                        | "impl"
                        | "dyn"
                        | "fn"
                ) =>
            {
                path += 1
            }
            _ => {}
        }
        if path > MAX_PATH_SYNTAX {
            return Err(too_deep());
        }
    }
    Ok(())
}

/// Parse, walk and drop syntax trees on the same bounded-stack worker.
fn on_parse_stack<T: Send>(work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("portable-parse".into())
            .stack_size(PARSE_STACK)
            .spawn_scoped(scope, work)
            .map_err(|_| Error::config("The policy checker is busy. Retry later."))?
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// Whether `name` is the key constant of a canonical rule, such as `PAYMENT_LIMIT`.
fn canonical_constant(name: &str) -> bool {
    name == name.to_uppercase()
        && allowit::RULES
            .iter()
            .any(|r| r.key() == name.to_lowercase())
}

/// How custom code reads one param with `allowit::param`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamRead {
    /// The read names its type, as in `let limit: u32 = allowit::param(ctx, "max_count");` or
    /// `allowit::param::<u64>(ctx, "max_count")`.
    Typed(String),
    /// The compiler infers the type from the use, or the type is not a plain name.
    Inferred,
}

/// The `allowit::param` reads with a string literal key, by key.
#[derive(Default)]
struct Reads(BTreeMap<String, Vec<ParamRead>>);

/// The key of an `allowit::param(ctx, "<key>")` call, or of a read with a canonical key constant,
/// such as `allowit::param(ctx, allowit::CALL_LIMIT)`.
fn param_key(call: &syn::ExprCall) -> Option<String> {
    if allowit_path(&call.func).as_deref() != Some("param") {
        return None;
    }
    match call.args.iter().nth(1)? {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(key),
            ..
        }) => Some(key.value()),
        expr => allowit_path(expr)
            .filter(|name| canonical_constant(name))
            .map(|name| name.to_lowercase()),
    }
}

/// The plain name of `ty`, such as `u32`.
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) if path.qself.is_none() => {
            path.path.get_ident().map(ToString::to_string)
        }
        _ => None,
    }
}

/// The type of a turbofish read, `allowit::param::<u64>(...)`, when it has a plain name.
fn turbofish(call: &syn::ExprCall) -> Option<String> {
    let syn::Expr::Path(path) = &*call.func else {
        return None;
    };
    let syn::PathArguments::AngleBracketed(arguments) = &path.path.segments.last()?.arguments
    else {
        return None;
    };
    match arguments.args.iter().collect::<Vec<_>>().as_slice() {
        [syn::GenericArgument::Type(ty)] => type_name(ty),
        _ => None,
    }
}

impl Reads {
    fn push(&mut self, key: String, read: Option<String>) {
        self.0
            .entry(key)
            .or_default()
            .push(read.map_or(ParamRead::Inferred, ParamRead::Typed));
    }
}

impl<'ast> syn::visit::Visit<'ast> for Reads {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (syn::Pat::Type(pattern), Some(init)) = (&local.pat, &local.init)
            && init.diverge.is_none()
            && let syn::Expr::Call(call) = &*init.expr
            && let Some(key) = param_key(call)
        {
            self.push(key, turbofish(call).or_else(|| type_name(&pattern.ty)));
            return;
        }
        syn::visit::visit_local(self, local);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some(key) = param_key(call) {
            self.push(key, turbofish(call));
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// The `allowit::param` reads of `source` by param key, so a value edit can stay within the
/// types that the policy reads.
pub fn param_reads(source: &str) -> Result<BTreeMap<String, Vec<ParamRead>>> {
    on_parse_stack(|| {
        use syn::visit::Visit;
        let mut reads = Reads::default();
        check_source_shape(source)?;
        let parsed = syn::parse_file(source)
            .map_err(|error| Error::usage(format!("The policy is not valid Rust: {error}.")))?;
        reads.visit_file(&parsed);
        Ok(reads.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_reads_name_their_types() {
        let source = "use allowit::prelude::*;\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    let limit: u32 = allowit::param(ctx, \"max_count\");\n    let large = allowit::param::<i128>(ctx, \"large\");\n    if ctx.request.amount > allowit::param(ctx, \"large\") {\n        return Err(AllowitError::PaymentLimitExceeded);\n    }\n    let pair: Option<u64> = allowit::param(ctx, \"pair\");\n    let calls: u32 = allowit::param(ctx, allowit::CALL_LIMIT);\n    Ok(())\n}\n";
        let reads = param_reads(source).unwrap();
        assert_eq!(reads["max_count"], [ParamRead::Typed("u32".into())]);
        assert_eq!(
            reads["large"],
            [ParamRead::Typed("i128".into()), ParamRead::Inferred]
        );
        assert_eq!(reads["pair"], [ParamRead::Inferred]);
        // A canonical key constant names its param.
        assert_eq!(reads["call_limit"], [ParamRead::Typed("u32".into())]);
        assert_eq!(reads.len(), 4);
    }

    #[test]
    fn invalid_or_deep_source_is_not_read_as_editable_parameters() {
        assert!(param_reads("pub fn execute( {").is_err());
        let too_deep = format!(
            "fn f() {{ let value = {}0{}; }}",
            "(".repeat(33),
            ")".repeat(33)
        );
        assert!(param_reads(&too_deep).is_err());
        let too_long_path = format!("fn f() {{ let value = {}true; }}", "!".repeat(161));
        assert!(param_reads(&too_long_path).is_err());
    }
}
