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
        && allowit_core::RULES
            .iter()
            .any(|r| r.key() == name.to_lowercase())
}

/// How custom code reads one param with `ctx.params[key].ok()` or `allowit::param`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamRead {
    /// The read names its type, as in `let limit: u32 = ctx.params["max_count"].ok();`,
    /// `let limit: u32 = allowit::param(ctx, "max_count");` or
    /// `allowit::param::<u64>(ctx, "max_count")`.
    Typed(String),
    /// The compiler infers the type from the use, or the type is not a plain name.
    Inferred,
}

/// The typed reads with a known key, by key.
#[derive(Default)]
struct Reads {
    reads: BTreeMap<String, Vec<ParamRead>>,
    /// The file's `const NAME: &str = "key";` items, by name.
    constants: BTreeMap<String, String>,
}

/// How many items and bindings declare each name, in every scope. A file-level constant names a
/// key only when nothing else declares its name, so no scope can shadow it.
#[derive(Default)]
struct Names(BTreeMap<String, usize>);

impl Names {
    fn declare(&mut self, name: &syn::Ident) {
        *self.0.entry(name.to_string()).or_default() += 1;
    }
}

impl<'ast> syn::visit::Visit<'ast> for Names {
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.declare(&item.ident);
        syn::visit::visit_item_const(self, item);
    }
    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        self.declare(&item.ident);
        syn::visit::visit_item_static(self, item);
    }
    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.declare(&item.ident);
        syn::visit::visit_impl_item_const(self, item);
    }
    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        self.declare(&item.ident);
        syn::visit::visit_trait_item_const(self, item);
    }
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.declare(&pattern.ident);
        syn::visit::visit_pat_ident(self, pattern);
    }
    fn visit_use_name(&mut self, name: &'ast syn::UseName) {
        self.declare(&name.ident);
    }
    fn visit_use_rename(&mut self, name: &'ast syn::UseRename) {
        self.declare(&name.rename);
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

/// The type of a turbofish, `::<u64>`, when it has a plain name.
fn turbofish(arguments: &syn::AngleBracketedGenericArguments) -> Option<String> {
    match arguments.args.iter().collect::<Vec<_>>().as_slice() {
        [syn::GenericArgument::Type(ty)] => type_name(ty),
        _ => None,
    }
}

/// The type of a turbofish read, `allowit::param::<u64>(...)`, when it has a plain name.
fn call_turbofish(call: &syn::ExprCall) -> Option<String> {
    let syn::Expr::Path(path) = &*call.func else {
        return None;
    };
    match &path.path.segments.last()?.arguments {
        syn::PathArguments::AngleBracketed(arguments) => turbofish(arguments),
        _ => None,
    }
}

/// `expr` without parentheses.
fn bare(mut expr: &syn::Expr) -> &syn::Expr {
    while let syn::Expr::Paren(paren) = expr {
        expr = &paren.expr;
    }
    expr
}

impl Reads {
    fn push(&mut self, key: String, read: Option<String>) {
        self.reads
            .entry(key)
            .or_default()
            .push(read.map_or(ParamRead::Inferred, ParamRead::Typed));
    }

    /// The key that `expr` names: a string literal, a canonical key constant such as
    /// `allowit::CALL_LIMIT`, or a `&str` constant of the file.
    fn key(&self, expr: &syn::Expr) -> Option<String> {
        match bare(expr) {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(key),
                ..
            }) => Some(key.value()),
            syn::Expr::Path(path) if path.qself.is_none() && path.path.get_ident().is_some() => {
                self.constants
                    .get(&path.path.get_ident()?.to_string())
                    .cloned()
            }
            expr => allowit_path(expr)
                .filter(|name| canonical_constant(name))
                .map(|name| name.to_lowercase()),
        }
    }

    /// The key of an `allowit::param(ctx, <key>)` call.
    fn param_key(&self, call: &syn::ExprCall) -> Option<String> {
        if allowit_path(&call.func).as_deref() != Some("param") {
            return None;
        }
        self.key(call.args.iter().nth(1)?)
    }

    /// The key and the turbofish type of a `<ctx>.params[<key>].ok()` read.
    fn ok_read(&self, expr: &syn::Expr) -> Option<(String, Option<String>)> {
        let syn::Expr::MethodCall(call) = expr else {
            return None;
        };
        let syn::Expr::Index(index) = bare(&call.receiver) else {
            return None;
        };
        let syn::Expr::Field(field) = bare(&index.expr) else {
            return None;
        };
        let params = matches!(&field.member, syn::Member::Named(name) if name == "params");
        if call.method != "ok" || !call.args.is_empty() || !params {
            return None;
        }
        Some((
            self.key(&index.index)?,
            call.turbofish.as_ref().and_then(turbofish),
        ))
    }
}

impl<'ast> syn::visit::Visit<'ast> for Reads {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let (syn::Pat::Type(pattern), Some(init)) = (&local.pat, &local.init)
            && init.diverge.is_none()
        {
            if let syn::Expr::Call(call) = &*init.expr
                && let Some(key) = self.param_key(call)
            {
                self.push(key, call_turbofish(call).or_else(|| type_name(&pattern.ty)));
                return;
            }
            if let Some((key, turbofish)) = self.ok_read(bare(&init.expr)) {
                self.push(key, turbofish.or_else(|| type_name(&pattern.ty)));
                return;
            }
        }
        syn::visit::visit_local(self, local);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Some(key) = self.param_key(call) {
            self.push(key, call_turbofish(call));
        }
        syn::visit::visit_expr_call(self, call);
    }
    fn visit_expr_reference(&mut self, reference: &'ast syn::ExprReference) {
        // `&ctx.params[key].ok()` passes the value itself, as a canonical rule takes it. The
        // rule's own bounds apply.
        if matches!(self.ok_read(bare(&reference.expr)), Some((_, None))) {
            return;
        }
        syn::visit::visit_expr_reference(self, reference);
    }
    fn visit_expr(&mut self, expr: &'ast syn::Expr) {
        if let Some((key, turbofish)) = self.ok_read(expr) {
            self.push(key, turbofish);
        }
        syn::visit::visit_expr(self, expr);
    }
}

/// The typed reads of `source` by param key, so a value edit can stay within the types that the
/// policy reads.
pub fn param_reads(source: &str) -> Result<BTreeMap<String, Vec<ParamRead>>> {
    on_parse_stack(|| {
        use syn::visit::Visit;
        let mut reads = Reads::default();
        check_source_shape(source)?;
        let parsed = syn::parse_file(source)
            .map_err(|error| Error::usage(format!("The policy is not valid Rust: {error}.")))?;
        let mut names = Names::default();
        names.visit_file(&parsed);
        for item in &parsed.items {
            if let syn::Item::Const(constant) = item
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(key),
                    ..
                }) = bare(&constant.expr)
                && names.0.get(&constant.ident.to_string()) == Some(&1)
            {
                reads
                    .constants
                    .insert(constant.ident.to_string(), key.value());
            }
        }
        reads.visit_file(&parsed);
        Ok(reads.reads)
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
    fn ok_reads_name_their_types_and_rule_arguments_keep_the_rule_bounds() {
        let source = "use allowit::prelude::*;\nconst FLOOR: &str = \"swap_output_floor\";\nconst SWAP_LIMIT: &str = \"swap_limit\";\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    allowit::payment_limit(ctx, &ctx.params[SWAP_LIMIT].ok())?;\n    allowit::call_limit(ctx, &ctx.params[allowit::CALL_LIMIT].ok())?;\n    let floor: i128 = ctx.params[FLOOR].ok();\n    let count: u32 = ctx.params[\"max_count\"].ok();\n    let wide = ctx.params[\"max_count\"].ok::<u64>();\n    if ctx.request.amount > ctx.params[\"large\"].ok() {\n        return Err(AllowitError::PaymentLimitExceeded);\n    }\n    let calls: u64 = ((ctx.params[allowit::CALL_LIMIT]).ok());\n    Ok(())\n}\n";
        let reads = param_reads(source).unwrap();
        assert_eq!(
            reads["swap_output_floor"],
            [ParamRead::Typed("i128".into())]
        );
        assert_eq!(
            reads["max_count"],
            [
                ParamRead::Typed("u32".into()),
                ParamRead::Typed("u64".into())
            ]
        );
        assert_eq!(reads["large"], [ParamRead::Inferred]);
        assert_eq!(reads["call_limit"], [ParamRead::Typed("u64".into())]);
        // A value passed to a rule records no typed read.
        assert_eq!(reads.len(), 4);
    }

    #[test]
    fn a_shadowed_key_constant_names_no_param() {
        let source = "use allowit::prelude::*;\nconst KEY: &str = \"wide\";\npub fn execute(ctx: &AllowitContext) -> allowit::Result {\n    const KEY: &str = \"narrow\";\n    let n: u32 = allowit::param(ctx, KEY);\n    let m: u32 = ctx.params[KEY].ok();\n    Ok(())\n}\n";
        assert!(param_reads(source).unwrap().is_empty());
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
