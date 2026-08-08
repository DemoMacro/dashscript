//! `.ts` translatability check — the middle layer of the three-layer
//! correctness chain (structure → translatability → `cargo check`).
//!
//! It reuses the translator's own mapping as the single source of truth: any
//! top-level statement [`super::functions::translate_statement`] cannot lower is
//! reported as a diagnostic, alongside the syntax errors `oxc_parser` already
//! surfaced. This answers "can this `.ts` become valid Rust?" — which
//! eslint-style rules cannot express, and which `oxc_linter` (not on crates.io)
//! is therefore not used for.
//!
//! A second pass walks the function body for **low-compatibility constructs**
//! ([`collect_unsupported`]), classifying each expression via [`super::classify`]
//! — the translator's single translatability table. A node the translator
//! cannot statically lower maps to [`super::classify::Mapping::Reject`] (a hard
//! `unsupported`) or [`super::classify::Mapping::DegradeEngine`] (the embedded
//! QuickJS engine runs it). Flagging them here reports them honestly rather
//! than letting the translator emit broken Rust that fails `cargo check`
//! (reported as `partial`); the conformance matrix then reflects what
//! DashScript can actually express rather than what merely parses.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentTarget, BindingIdentifier, BindingPattern, ChainElement, ChainExpression,
    Declaration, Expression, ForStatementInit, Function, ObjectPropertyKind, Statement,
    UnaryOperator,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType, Span};

use super::classify::{self, ClassifyCtx, LocalKind, Mapping};
use super::name_table;
use super::{analysis, functions, registry, FileRole};

// The set of prototype borrows `is_borrow_call` whitelists depends on the
// caller (`check` vs the engine detector). Rather than thread a `for_engine`
// bool through every recursive `collect_*` call, a thread-local flag carries
// it: `program_uses_engine` sets it for the duration of its walk
// (`EngineScope` resets it on drop — even on panic), `check` leaves it at the
// default `false`. Per-thread, so the conformance harness's parallel workers
// each carry their own.
thread_local! {
    static FOR_ENGINE: Cell<bool> = const { Cell::new(false) };
}

/// Traverse state threaded through the unsupported-construct walk — the bits a
/// context-dependent classification reads: whether the current expression sits
/// inside a loop (a looped `re.exec` needs the engine), and which locals bind
/// to a known value kind (a `.test`/`.exec` on a non-string local, or a
/// `compare`/`from` on a Temporal local — see [`classify::LocalKind`]).
/// Threading it explicitly (instead of the prior `IN_LOOP`/`NON_STRING_VARS`
/// thread-locals) keeps the walk self-contained and parallel-safe without
/// per-thread globals.
struct WalkState {
    in_loop: bool,
    local_kinds: HashMap<String, LocalKind>,
    /// Whether `DegradeEngine` classifications (and a runtime-`typeof`
    /// operand) emit a diagnostic. `ds lint` ([`check`]) sets this true — a
    /// degrade is information a user acts on (the function runs under the
    /// embedded QuickJS). The conformance harness sets it false via
    /// [`check_reject_only`]: a degrade is *not* a translatability failure
    /// (the function still lowers, via the engine), so the fixture proceeds
    /// through the compile path rather than short-circuiting as `unsupported`.
    /// [`program_engine_sites`] sets it true — it counts degrade diagnostics
    /// to detect which functions to emit into the engine.
    include_degrades: bool,
}

/// RAII guard: constructed to mark an engine-path detection in progress;
/// resets `FOR_ENGINE` on drop so a panic mid-walk cannot leak the flag into a
/// later `check` on the same thread (which would then wrongly whitelist Array
/// prototype borrows).
struct EngineScope;

impl Drop for EngineScope {
    fn drop(&mut self) {
        FOR_ENGINE.with(|c| c.set(false));
    }
}

/// Check `.ts` source for translatability. Returns syntax errors from
/// `oxc_parser` plus one diagnostic per construct the translator cannot map —
/// both unmapped top-level statements and low-compatibility constructs buried
/// inside a function body. An empty result means the file lowers to valid
/// Rust (as far as DashScript can tell — `cargo check` is still the final
/// arbiter).
pub(super) fn check(source: &str) -> Vec<OxcDiagnostic> {
    check_as(source, FileRole::BinEntry)
}

/// Role-aware translatability check — see [`check`]. [`FileRole::Module`]
/// additionally flags top-level executable statements: a module declares an
/// API, it does not run, so a top-level `const`/expression/control-flow has no
/// entry to land in. Declarations (function/class/interface/type/import/export)
/// pass under either role.
pub(super) fn check_as(source: &str, role: FileRole) -> Vec<OxcDiagnostic> {
    check_as_inner(source, role, true)
}

/// Like [`check_as`] but drops `DegradeEngine` (and runtime-`typeof`)
/// diagnostics. For the conformance harness: a degrade is a compile-path
/// fallback (the function still lowers, via the embedded QuickJS), not a
/// translatability failure, so a degraded fixture proceeds through
/// `cargo build` + run rather than short-circuiting as `unsupported`. Only a
/// hard `Reject` short-circuits.
pub(super) fn check_reject_only(source: &str, role: FileRole) -> Vec<OxcDiagnostic> {
    check_as_inner(source, role, false)
}

fn check_as_inner(source: &str, role: FileRole, include_degrades: bool) -> Vec<OxcDiagnostic> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    // Layer 1 — structure: oxc_parser syntax errors.
    let mut diagnostics = ret.diagnostics.into_vec();

    // Build the per-symbol `NameTable` once. `check` only drives
    // `translate_statement` to ask "is this top-level statement mapped?" — it
    // never relies on the table's disambiguation (that is stage 1.3) — but the
    // translator now resolves every identifier through it, so the same table
    // the emit path uses must be built here too.
    let program = allocator.alloc(ret.program);
    let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
    let names = name_table::build(sret.semantic.scoping());

    // Layer 2 — translatability: the translator is the source of truth (its
    // `None` means "not mapped"); the match only adds a human message + span.
    let registry = registry::build_registry(&program.body, &names);
    // Top-level `let` bindings mutated from a top-level function cannot lower
    // to an immutable `OnceLock` (they need a `thread_local!` `RefCell`, B3-2),
    // so the lazy-static candidate check excludes them.
    let mutable_top_level = functions::mutable_top_level_names(&program.body, &names, &registry);
    let mut state = WalkState {
        in_loop: false,
        local_kinds: HashMap::new(),
        include_degrades,
    };
    for stmt in &program.body {
        // `export {}` is a standard TS module marker (no declaration, no
        // specifiers): it makes the file a module so its declarations stay
        // file-local instead of polluting the global scope. The translator
        // lowers it to nothing, so skip it — otherwise the empty translate
        // below would flag it as an unmapped statement.
        if is_module_marker(stmt) {
            continue;
        }
        if functions::is_executable_top_level(stmt) {
            if matches!(role, FileRole::Module) {
                // Module semantics (arch decision point 8): a module only
                // declares. But a const-expr `const` (number/boolean/string
                // literal) lowers to a crate `const` item — no side effect, no
                // `fn main` needed — so a module may carry it. Only a genuine
                // side-effecting executable (a non-const-expr binding, an
                // expression statement, control flow, a throw) is unsupported:
                // it has no entry to run in.
                let promotable = if let Statement::VariableDeclaration(v) = stmt {
                    functions::promotable_const_info(v, &names, &mutable_top_level).is_some()
                } else {
                    false
                };
                // A non-const-expr `const` or non-mutated `let` with an
                // inferable type (an object, a regex) lowers to a lazy static
                // (OnceLock + accessor fn), so a module may carry it too — no
                // `fn main` needed.
                let lazy_static =
                    functions::lazy_static_candidate(stmt, &mutable_top_level, &names);
                // A mutable module-global value `let` (rebound/mutated from a
                // function) lowers to a thread-local `RefCell` + get/set
                // accessors (B3-2) — no `fn main` needed, so a module may carry it.
                let mutable_static =
                    functions::mutable_static_candidate(stmt, &mutable_top_level, &names);
                if promotable || lazy_static || mutable_static {
                    collect_unsupported(stmt, &mut diagnostics, &mut state);
                } else {
                    // Unhoistable top-level executable (a `for` loop filling a
                    // Map at load time, a `let` holding a function value) →
                    // whole-module degrade (degrade-over-reject; arch decision
                    // point 8): the translator runs every top-level function
                    // under the engine, and the module source carries these
                    // statements. A conformance check (`include_degrades =
                    // false`) drops this so the fixture proceeds through the
                    // compile path; `ds lint` surfaces it as information.
                    if state.include_degrades {
                        diagnostics.push(err(
                            "module top-level executable degrades to the engine — a \
                             module declares, so this statement runs under the embedded \
                             QuickJS rather than in a `fn main`",
                            stmt.span(),
                        ));
                    }
                }
            } else {
                // Pure-TS execution semantics: executable statements (a `const`,
                // an expression, control flow, a throw) run in source order inside
                // the implicit `fn main`, so they are legitimate top-level — not
                // unmapped. Still walk for low-compatibility constructs inside.
                collect_unsupported(stmt, &mut diagnostics, &mut state);
            }
            continue;
        }
        if functions::translate_statement(stmt, &registry, &names).is_empty() {
            diagnostics.push(unmapped_top_level(stmt));
        }
        // Low-compatibility constructs inside the body — see
        // [`collect_unsupported`].
        collect_unsupported(stmt, &mut diagnostics, &mut state);
    }
    // A top-level `function` reading a top-level `const`/`let` would close over
    // a binding living in `fn main` — impossible for a Rust fn item. This is an
    // entry-file concern only: a module file has no `fn main`, so every
    // top-level binding lowers to a module item (a `const`, or a `OnceLock`
    // accessor for a non-const-expr `const`/non-mutated `let`) a function may
    // reference freely. Hoisting entry bindings to module items is a later
    // batch (B3-1b/B3-2); until then surface the escape honestly rather than
    // letting it fail `cargo check` as a partial.
    if !matches!(role, FileRole::Module) {
        let escaped_lazy = functions::escaped_lazy_static_names(
            &program.body,
            &names,
            &registry,
            &mutable_top_level,
        );
        // `dynamic_fns` lets `check_escape` skip degraded functions — their
        // bodies run under the engine, where a top-level binding capture is
        // legal (no Rust fn-item restriction). And when a dynamic construct
        // sits at top level (`top_level_dynamic`), the *whole* program runs
        // under the engine, so `check_escape` does not apply at all — skip it.
        let sites = program_engine_sites(program);
        if !sites.top_level_dynamic {
            check_escape(
                &program.body,
                &names,
                &registry,
                &mutable_top_level,
                &escaped_lazy,
                &sites.dynamic_fns,
                &mut diagnostics,
            );
        }
    }
    diagnostics
}

/// Flag a top-level `function` whose body reads a top-level `const`/`let`
/// binding. Such a binding is collected into the implicit `fn main` (it runs in
/// source order), so a function reading it would have to close over a `main`
/// local — impossible for a Rust fn item. Hoisting top-level bindings to module
/// items is a later batch; until then this surfaces the gap as `unsupported`
/// rather than letting it fail `cargo check` as a partial. Keyed by the
/// per-symbol Rust name (`analysis::analyze`'s key), so the rare shadow — a
/// same-named local declared inside the function — can false-positive; the
/// common case (no name clash) is sound.
fn check_escape(
    program_body: &[Statement],
    names: &name_table::NameTable,
    registry: &registry::TypeRegistry,
    mutable_names: &HashSet<String>,
    escaped_lazy: &HashSet<String>,
    dynamic_fns: &HashSet<String>,
    out: &mut Vec<OxcDiagnostic>,
) {
    // A const-expr `const` number/boolean literal referenced from a top-level
    // function is promoted to a crate-level `const` item (escape promotion,
    // A3) — that escape is legal. Everything else (a `let`/`var`, a string, a
    // runtime initializer) still cannot be captured by a Rust fn item, so it
    // stays `unsupported`. `promoted_const_names` is the single source of truth
    // the translator also uses, so `check` and emit agree on what is hoisted.
    let promoted = functions::promoted_const_names(program_body, names, registry, mutable_names);
    // A non-const-expr `const`/non-mutated `let` referenced from a top-level
    // function hoists to a lazy static (OnceLock + accessor, B3-1b) — that
    // escape is legal too. `escaped_lazy_static_names` is the single source of
    // truth the translator also uses, so `check` and emit agree on what hoists.
    let flaggable: HashSet<String> = program_body
        .iter()
        .filter_map(|s| match s {
            Statement::VariableDeclaration(v) => Some(v),
            _ => None,
        })
        .flat_map(|v| v.declarations.iter())
        .filter_map(|d| match &d.id {
            BindingPattern::BindingIdentifier(id) => Some(names.of_binding(id).to_string()),
            _ => None,
        })
        .filter(|n| !promoted.contains(n) && !escaped_lazy.contains(n))
        .collect();
    if flaggable.is_empty() {
        return;
    }
    for stmt in program_body {
        let Statement::FunctionDeclaration(f) = stmt else {
            continue;
        };
        // A degraded function's body runs under the embedded engine (JS),
        // where it closes over top-level bindings the usual JS way — the Rust
        // fn-item capture restriction does not apply. Skip it: the engine
        // finds both the function and the binding in `__DS_MODULE_JS`.
        if f.id
            .as_ref()
            .map(|id| dynamic_fns.contains(id.name.as_str()))
            .unwrap_or(false)
        {
            continue;
        }
        let Some(body) = &f.body else { continue };
        let analysis = analysis::analyze(
            &body.statements,
            names,
            &registry.mut_methods,
            &registry.ref_params,
        );
        // A read *or* a write (a rebind `n = …` or a member mutation `n.x =
        // …`) of a flaggable binding from a function is an escape — both close
        // over an `fn main` local. Use counts cover reads; `mutated`/
        // `member_mutated` cover writes (the write-only case a read-only check
        // would miss).
        let mut escapes = analysis
            .use_counts
            .keys()
            .chain(analysis.mutated.iter())
            .chain(analysis.member_mutated.iter());
        if escapes.any(|k| flaggable.contains(k)) {
            out.push(err(
                "a `let`/`var` or non-literal binding referenced from a top-level \
                 function is not yet supported — use a `const` number/boolean \
                 literal, move the binding into the function, or call the function \
                 from the top level",
                f.span,
            ));
        }
    }
}

/// Per-function engine degradation sites: which top-level functions contain a
/// construct the static translator cannot lower (their bodies will run under
/// QuickJS via `__ds::engine::call_fn`), and whether any dynamic construct sits
/// at top level — outside any function — which still needs the whole-program
/// `run` path (there is no function boundary to rewrite).
#[derive(Default)]
pub(super) struct EngineSites {
    /// TS names of top-level `function` declarations whose body contains a
    /// low-compatibility construct (a `DegradeEngine` classification).
    pub dynamic_fns: HashSet<String>,
    /// A dynamic construct at top level (not inside any function) — no function
    /// boundary to rewrite, so the whole program falls back to `run`.
    pub top_level_dynamic: bool,
}

/// A top-level `function` declaration a statement carries — a bare
/// `function f() {}` or an `export function f() {}`. `None` for anything else
/// (a non-function statement, an `export` of a class/variable/re-export). Used
/// by the engine-site walk so an exported dynamic function degrades the same
/// way a non-exported one does.
///
/// Also covers a const-arrow fn (`const f = <T>(x): ret => …` / `export const f
/// = …`), which lowers to a `fn` item too. The const binding names the
/// function (the arrow has no `id`), so [`TopLevelFn::Arrow`] carries that
/// `BindingIdentifier`. This is the B6d #312 extension: a const-arrow fn whose
/// signature carries an unmappable type (`unknown`/indexed access/…) degrades
/// to the engine the same way a `function` declaration does.
pub(super) enum TopLevelFn<'a> {
    /// A `function f() {}` / `export function f() {}` — `f.id` carries the name.
    Decl(&'a Function<'a>),
    /// A `const f = () => …` / `export const f = () => …` — the const binding
    /// names the function (the arrow has no `id`), so it is carried here.
    Arrow(
        &'a oxc_ast::ast::ArrowFunctionExpression<'a>,
        &'a BindingIdentifier<'a>,
    ),
}

impl<'a> TopLevelFn<'a> {
    /// The TS source name — `f.id` for a declaration, the const binding name
    /// for a const-arrow fn. `None` only for an anonymous `function` expression
    /// (no top-level binding), which cannot be a degrade target.
    pub(super) fn ts_name(&self) -> Option<&str> {
        match self {
            TopLevelFn::Decl(f) => f.id.as_ref().map(|id| id.name.as_str()),
            TopLevelFn::Arrow(_, id) => Some(id.name.as_str()),
        }
    }
}

/// Lift a statement to a [`TopLevelFn`] — a `function f`, an `export function
/// f`, or a const-arrow `const f = () => …` / `export const f = () => …`. The
/// const-arrow case is the B6d #312 extension to per-function engine degrade.
/// `None` for any other statement (a non-function declaration, an executable
/// statement, an `export` of a class/variable/re-export that is not a const
/// arrow).
pub(super) fn top_level_function<'a>(stmt: &'a Statement<'a>) -> Option<TopLevelFn<'a>> {
    match stmt {
        Statement::FunctionDeclaration(f) => Some(TopLevelFn::Decl(f)),
        Statement::ExportNamedDeclaration(e) => match &e.declaration {
            Some(Declaration::FunctionDeclaration(f)) => Some(TopLevelFn::Decl(f)),
            Some(Declaration::VariableDeclaration(v)) => const_arrow_top_level(v),
            _ => None,
        },
        Statement::VariableDeclaration(v) => const_arrow_top_level(v),
        _ => None,
    }
}

/// The shared const-arrow lifter for a `VariableDeclaration` (the body of
/// `const f = () => …` and `export const f = () => …`). A single-declarator
/// const arrow binds one `BindingIdentifier` to one `ArrowFunctionExpression`;
/// anything else (a `let`, a multi-declarator, a non-arrow initializer, a
/// destructured pattern) yields `None`.
fn const_arrow_top_level<'a>(
    v: &'a oxc_ast::ast::VariableDeclaration<'a>,
) -> Option<TopLevelFn<'a>> {
    if v.declarations.len() != 1 {
        return None;
    }
    let d = &v.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    match d.init.as_ref()? {
        Expression::ArrowFunctionExpression(arrow) => Some(TopLevelFn::Arrow(arrow, id)),
        _ => None,
    }
}

/// Walk the program once and split the unsupported-construct diagnostics by
/// where they land: inside a top-level function (a per-function degradation
/// site) or outside one (whole-program fallback). The split reuses the exact
/// same `collect_unsupported` walk that flags these as `unsupported` in
/// `ds lint` — one source of truth for what the engine covers.
pub(super) fn program_engine_sites(program: &oxc_ast::ast::Program) -> EngineSites {
    // For the duration of this walk, `is_borrow_call` whitelists every prototype
    // borrow the translator *attempts* (String + Array), so a borrow the
    // translator can lower is not needlessly stolen by the engine. The scope
    // guard resets the flag on drop — even on panic.
    FOR_ENGINE.with(|c| c.set(true));
    let _scope = EngineScope;
    let mut state = WalkState {
        in_loop: false,
        local_kinds: HashMap::new(),
        include_degrades: true,
    };
    let mut diags = Vec::new();
    let mut sites = EngineSites::default();
    for stmt in &program.body {
        // Record the diagnostic count before recursing into this statement;
        // any new diagnostic it produces landed inside it. A function
        // declaration that adds a diagnostic has the construct in its body
        // (a per-function site); anything else that adds one has no function
        // boundary to rewrite (whole-program fallback).
        let before = diags.len();
        collect_unsupported(stmt, &mut diags, &mut state);
        let body_dynamic = diags.len() > before;
        // A function whose signature carries a type the static translator
        // cannot express (`unknown`/indexed access/…) degrades too — the `_`
        // would fail cargo check at the signature, not the body.
        // `classify_function_signature` / `classify_arrow_signature` is the
        // type-driven trigger; the body construct walk above is the AST-driven
        // one. The const-arrow case (B6d #312) is reached here through the
        // `TopLevelFn::Arrow` variant — its name comes from the const binding.
        let sig_dynamic = match top_level_function(stmt) {
            Some(TopLevelFn::Decl(f)) => matches!(
                classify::classify_function_signature(f),
                Mapping::DegradeEngine(_)
            ),
            Some(TopLevelFn::Arrow(arrow, _)) => matches!(
                classify::classify_arrow_signature(arrow),
                Mapping::DegradeEngine(_)
            ),
            None => false,
        };
        if body_dynamic || sig_dynamic {
            if let Some(tlf) = top_level_function(stmt) {
                if let Some(name) = tlf.ts_name() {
                    sites.dynamic_fns.insert(name.to_string());
                }
            } else {
                sites.top_level_dynamic = true;
            }
        }
    }
    sites
}

/// True when the program needs the engine at all — either a per-function site
/// or a top-level dynamic construct. This is the conformance-oracle gate: when
/// it returns true, the whole program runs under QuickJS via `run`. The
/// translator's own routing uses the richer [`program_engine_sites`] to pick
/// per-function vs whole-program; this collapses that to a single bool.
pub(super) fn program_uses_engine(program: &oxc_ast::ast::Program) -> bool {
    let sites = program_engine_sites(program);
    sites.top_level_dynamic || !sites.dynamic_fns.is_empty()
}

/// True for `export {}` — an empty named export (no declaration, no
/// specifiers). It is the standard TS module marker that makes a file a
/// module; the translator lowers it to nothing, so it is not a translatability
/// gap. `export { x }` re-exports and `export ... from` are not matched (the
/// translator does not support them).
fn is_module_marker(stmt: &Statement) -> bool {
    matches!(
        stmt,
        Statement::ExportNamedDeclaration(exp)
            if exp.declaration.is_none() && exp.specifiers.is_empty()
    )
}

/// A human message + span for a top-level statement the translator skips.
fn unmapped_top_level(stmt: &Statement) -> OxcDiagnostic {
    match stmt {
        Statement::ImportDeclaration(s) => err(
            "this module import could not be resolved — use `cargo:<crate>` \
             for a Rust crate, `./<file>` for a local module, or a bare npm \
             package name resolved via `node_modules`",
            s.span,
        ),
        Statement::ExportNamedDeclaration(s) => err("module `export` is not supported yet", s.span),
        Statement::ExportDefaultDeclaration(s) => err(
            "`export default <expression>` is not supported — use a default function or class",
            s.span,
        ),
        Statement::ExportAllDeclaration(s) => err("module `export *` is not supported yet", s.span),
        Statement::TSEnumDeclaration(s) => err(
            "this TypeScript `enum` has a member whose value is not a literal \
             (a computed name or a non-literal initializer) — only \
             literal-initialized or auto-incrementing numeric/string members \
             are supported",
            s.span,
        ),
        _ => OxcDiagnostic::error("this top-level statement cannot be translated to Rust"),
    }
}

/// Walk a statement (and every expression nested inside it) collecting one
/// diagnostic per low-compatibility construct — each expression classified by
/// [`super::classify`]. Recurses through every statement/expression kind the
/// translator itself walks (mirroring `analysis::walk_stmt`), so a construct
/// buried in a loop, branch, or callback is still surfaced. Unfamiliar kinds
/// fall through silently (a missed construct only means it stays `partial`,
/// not a false `unsupported`).
fn collect_unsupported(stmt: &Statement, out: &mut Vec<OxcDiagnostic>, state: &mut WalkState) {
    // A top-level `function` (bare or `export`ed) — recurse its body so a
    // low-compatibility construct inside it surfaces, and flag a nested
    // `function` declaration in it. (`program_engine_sites` reads this walk's
    // diagnostic count to decide per-function degradation, so an exported
    // dynamic function must add a diagnostic the same way a bare one does.)
    // The const-arrow case (`const f = () => …`) recurses its arrow body the
    // same way (B6d #312 extension).
    if let Some(f) = top_level_function(stmt) {
        let body: Option<&oxc_ast::ast::FunctionBody> = match f {
            TopLevelFn::Decl(func) => func.body.as_deref(),
            TopLevelFn::Arrow(arrow, _) => Some(arrow.body.as_ref()),
        };
        if let Some(body) = body {
            for s in &body.statements {
                // A nested `function` declaration now lowers to a Rust nested
                // fn item (see `translate_stmt`), so it is no longer flagged
                // here. `collect_unsupported(s)` still recurses into a nested
                // fn body so any unmappable construct inside (reflection,
                // async, …) surfaces as usual.
                collect_unsupported(s, out, state);
            }
        }
        return;
    }
    match stmt {
        Statement::BlockStatement(b) => collect_unsupported_stmts(&b.body, out, state),
        // `try { … } catch (e) { … }` — recurse the try block, the handler
        // body, and the optional `finally`, so a construct inside the handler
        // (`e.constructor.name`) or the try body is still surfaced.
        Statement::TryStatement(t) => {
            collect_unsupported_stmts(&t.block.body, out, state);
            if let Some(handler) = &t.handler {
                collect_unsupported_stmts(&handler.body.body, out, state);
            }
            if let Some(fin) = &t.finalizer {
                collect_unsupported_stmts(&fin.body, out, state);
            }
        }
        Statement::ExpressionStatement(es) => collect_expr(&es.expression, out, state),
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    // Record the value kind a binding's initializer resolves
                    // to, so a later context-dependent classification reads it:
                    // a plainly non-string literal (number/boolean/object/
                    // array) routes a `.test(x)`/`.exec(x)` to the engine (ES
                    // coerces via ToString, which regress taking `&str` cannot
                    // express); a `Temporal.<Type>.from(…)`/`new …(…)` routes
                    // `compare(a, b)`/`from(arg)` through the static
                    // `temporal_rs` mapping only when the arg genuinely is that
                    // Temporal type. A function-call or identifier initializer
                    // of unknown type is left unrecorded (no false route).
                    if let BindingPattern::BindingIdentifier(id) = &d.id {
                        let kind = match init {
                            Expression::StringLiteral(_) => Some(LocalKind::String),
                            Expression::NumericLiteral(_)
                            | Expression::BooleanLiteral(_)
                            | Expression::ObjectExpression(_)
                            | Expression::ArrayExpression(_) => Some(LocalKind::NonString),
                            // `new <Ctor>(…)` (an Identifier callee) pins the
                            // binding to that ctor, so a later `x instanceof
                            // <Ctor>` folds statically. A member callee
                            // (`new Temporal.X(…)`) falls through to the
                            // Temporal arm below; an engine-value-global `new`
                            // (`new Date()`) still records here, but its
                            // enclosing function already degrades via the
                            // `NewExpression` classify arm, so the entry is
                            // never read at an `instanceof`.
                            Expression::NewExpression(n) => match &n.callee {
                                Expression::Identifier(id) => {
                                    Some(LocalKind::Ctor(id.name.as_str().to_string()))
                                }
                                _ => None,
                            },
                            _ => super::builtins::temporal_init_type(init).map(LocalKind::Temporal),
                        };
                        if let Some(kind) = kind {
                            state.local_kinds.insert(id.name.as_str().to_string(), kind);
                        }
                    }
                    collect_expr(init, out, state);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_expr(&if_stmt.test, out, state);
            collect_unsupported(&if_stmt.consequent, out, state);
            if let Some(alt) = &if_stmt.alternate {
                collect_unsupported(alt, out, state);
            }
        }
        Statement::WhileStatement(w) => {
            collect_loop_expr(&w.test, out, state);
            collect_loop_body(&w.body, out, state);
        }
        Statement::DoWhileStatement(dw) => {
            collect_loop_body(&dw.body, out, state);
            collect_loop_expr(&dw.test, out, state);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    if let Some(i) = &d.init {
                        collect_expr(i, out, state);
                    }
                }
            }
            if let Some(test) = &f.test {
                collect_loop_expr(test, out, state);
            }
            if let Some(update) = &f.update {
                collect_loop_expr(update, out, state);
            }
            collect_loop_body(&f.body, out, state);
        }
        Statement::ForOfStatement(fo) => collect_loop_body(&fo.body, out, state),
        Statement::ForInStatement(fi) => collect_loop_body(&fi.body, out, state),
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                collect_expr(arg, out, state);
            }
        }
        Statement::ThrowStatement(t) => collect_expr(&t.argument, out, state),
        Statement::SwitchStatement(sw) => {
            collect_expr(&sw.discriminant, out, state);
            for c in &sw.cases {
                for s in &c.consequent {
                    collect_unsupported(s, out, state);
                }
            }
        }
        _ => {}
    }
}

/// Walk an assignment's left-hand target so a reflection nested in the lvalue
/// is surfaced — `obj[Symbol.X] = v`, `Array.prototype[k] = v`, or
/// `Array.prototype.foo = v`. The target's own verdict (`prototype` mutation,
/// a match-result field write, `<re>.lastIndex = …` write) comes from
/// [`super::classify::classify_assignment_target`]; the receiver and index
/// expressions are then recursed so a reflection buried there is not missed.
/// A plain `xs[i] = v` / `obj.f = v` adds nothing (no reflection), so
/// legitimate mutation stays supported.
fn collect_assignment_target(
    target: &AssignmentTarget,
    out: &mut Vec<OxcDiagnostic>,
    state: &mut WalkState,
) {
    match target {
        AssignmentTarget::ComputedMemberExpression(cm) => {
            match classify::classify_assignment_target(target) {
                Mapping::Reject(msg) => out.push(err(msg, cm.span)),
                Mapping::DegradeEngine(msg) => {
                    if state.include_degrades {
                        out.push(err(msg, cm.span));
                    }
                }
                Mapping::Mapped => {}
            }
            collect_expr(&cm.object, out, state);
            collect_expr(&cm.expression, out, state);
        }
        AssignmentTarget::StaticMemberExpression(sm) => {
            match classify::classify_assignment_target(target) {
                Mapping::Reject(msg) => out.push(err(msg, sm.span)),
                Mapping::DegradeEngine(msg) => {
                    if state.include_degrades {
                        out.push(err(msg, sm.span));
                    }
                }
                Mapping::Mapped => {}
            }
            collect_expr(&sm.object, out, state);
        }
        _ => {}
    }
}

/// Walk a slice of statements — the shared spine of [`collect_unsupported`]'s
/// block-shaped arms (a BlockStatement, a function/arrow body, try/catch
/// bodies).
fn collect_unsupported_stmts(
    stmts: &[Statement],
    out: &mut Vec<OxcDiagnostic>,
    state: &mut WalkState,
) {
    for s in stmts {
        collect_unsupported(s, out, state);
    }
}

/// Walk a loop body with [`WalkState::in_loop`] set, so a `re.exec(…)` inside
/// (the test262 `do { m = re.exec(s); … } while (1)` idiom) routes to the
/// engine — regress is stateless, so the loop would re-find the same match
/// every iteration (an infinite loop the harness times out at 30s).
fn collect_loop_body(body: &Statement, out: &mut Vec<OxcDiagnostic>, state: &mut WalkState) {
    let prev = state.in_loop;
    state.in_loop = true;
    collect_unsupported(body, out, state);
    state.in_loop = prev;
}

/// Walk a loop's per-iteration expression (a `while`/`do-while` test, or a
/// `for` test/update) with [`WalkState::in_loop`] set, so a `re.exec(…)` in the
/// condition — `while (re.exec(s) !== null)` — routes to the engine like one in
/// the body. (A `for` init is walked normally: it runs once, so a single
/// `.exec` there stays on the regress path.)
fn collect_loop_expr(expr: &Expression, out: &mut Vec<OxcDiagnostic>, state: &mut WalkState) {
    let prev = state.in_loop;
    state.in_loop = true;
    collect_expr(expr, out, state);
    state.in_loop = prev;
}

/// Classify the expression at `expr` via [`super::classify`] (turning a
/// non-`Mapped` verdict into a diagnostic), then recurse into its children. A
/// `typeof` operand is **not** recursed: `typeof` has its own mapping (a global
/// constructor → `"function"`), so `typeof Symbol`/`typeof Proxy` must stay
/// supported rather than tripping the identifier rule.
fn collect_expr(expr: &Expression, out: &mut Vec<OxcDiagnostic>, state: &mut WalkState) {
    let ctx = ClassifyCtx {
        in_loop: state.in_loop,
        local_kinds: &state.local_kinds,
    };
    match classify::classify_expr(expr, &ctx) {
        // A hard `unsupported` — always a diagnostic (the construct has no
        // DashScript mapping, not even via the engine).
        Mapping::Reject(msg) => out.push(err(msg, expr.span())),
        // A degrade is a translatability *fallback*, not a failure: the
        // function still lowers via the embedded QuickJS. `ds lint` reports
        // it (so the user sees which functions run under the engine); the
        // conformance harness skips it via `include_degrades` so the fixture
        // proceeds through the compile path instead of short-circuiting.
        Mapping::DegradeEngine(msg) => {
            if state.include_degrades {
                out.push(err(msg, expr.span()));
            }
        }
        Mapping::Mapped => {}
    }
    match expr {
        Expression::UnaryExpression(u) => {
            if matches!(u.operator, UnaryOperator::Typeof) {
                // `typeof` resolves to a static type string only for literals
                // and known globals; a runtime operand (user identifier, member
                // access, call) needs the engine's runtime type string. A
                // degrade (not a hard reject) — gated by `include_degrades`
                // like every other `DegradeEngine` verdict.
                if state.include_degrades
                    && super::expressions::typeof_operand_is_runtime(&u.argument)
                {
                    out.push(err(
                        "`typeof` of a runtime value (a user identifier, member access, or call) \
                         has no static lowering — the function runs under the engine",
                        u.span(),
                    ));
                }
            } else {
                collect_expr(&u.argument, out, state);
            }
        }
        Expression::BinaryExpression(b) => {
            collect_expr(&b.left, out, state);
            // `x instanceof T` — `T` is a constructor *name* (a type), not a
            // value reference; recursing it would trip the static-only-global
            // rule for `Object`/`Array`/… and wrongly degrade a function whose
            // `instanceof` classify already folded statically. The receiver
            // (left) is still classified.
            if !matches!(b.operator, oxc_syntax::operator::BinaryOperator::Instanceof) {
                collect_expr(&b.right, out, state);
            }
        }
        Expression::LogicalExpression(l) => {
            collect_expr(&l.left, out, state);
            collect_expr(&l.right, out, state);
        }
        Expression::ConditionalExpression(c) => {
            collect_expr(&c.test, out, state);
            collect_expr(&c.consequent, out, state);
            collect_expr(&c.alternate, out, state);
        }
        Expression::CallExpression(c) => {
            // A global-object static call (`Math.floor(x)`, `Array.isArray(x)`,
            // `Object.keys(m)`, `JSON.parse(s)`) takes the global name only as
            // the call's receiver — not as a value reference. Don't recurse the
            // callee (its receiver would otherwise trip the identifier rule);
            // recurse the arguments. A `function`-expression callee/argument is
            // already flagged by `classify` above, so it is not re-checked here.
            if !is_global_object_callee(&c.callee) && !is_borrow_call(&c.callee) {
                collect_expr(&c.callee, out, state);
            }
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    // A global-object value as an argument
                    // (`Object.isExtensible(JSON)`, `Object.isExtensible(Array.prototype)`)
                    // is often an ignored param of a no-op method — the static
                    // call above already resolved it — so skip the value-reference
                    // rule on it; these stay supported. Other args are scanned.
                    if !is_global_object_value(e) {
                        collect_expr(e, out, state);
                    }
                }
            }
        }
        // `new X(…)` — recurse the constructor and args so `new Proxy(…)` /
        // `new Symbol(…)` trip the identifier rule. A global-object constructor
        // (`new Map()`, `new Set()`) is mapped, so its receiver is skipped.
        Expression::NewExpression(n) => {
            // `new <engine-value-global>(…)` — classify's `NewExpression` arm
            // owns the verdict for these (`new Promise(executor)` → Mapped;
            // `new Proxy()`/`new DataView()`/… → Degrade), so the bare callee
            // is a constructor name, not a value reference. Recursing it would
            // either push a false-positive bare-identifier degrade (the mapped
            // `Promise` ctor) or a redundant one (the degrade classify already
            // recorded). `is_global_object_callee` already skips static-only
            // ctors (`new Map()`); this extends the skip to engine-value-global
            // ctors. (`is_global_object_callee` is shared with `CallExpression`,
            // where a bare `Promise(x)` must still degrade — so the gate lives
            // here, not in that helper.)
            let callee_is_engine_global = matches!(
                &n.callee,
                Expression::Identifier(id)
                    if super::globals::is_engine_value_global(id.name.as_str())
            );
            if !callee_is_engine_global && !is_global_object_callee(&n.callee) {
                collect_expr(&n.callee, out, state);
            }
            for arg in &n.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_expr(e, out, state);
                }
            }
        }
        // `(x) => { … }` / `(x) => e` — recurse the arrow body so a construct
        // buried in a callback (`xs.forEach(x => x instanceof B)`) is surfaced.
        // oxc wraps even a concise body as a FunctionBody whose single statement
        // is an ExpressionStatement.
        Expression::ArrowFunctionExpression(a) => {
            collect_unsupported_stmts(&a.body.statements, out, state);
        }
        // `(function () { … })()` — a function expression's body is walked
        // too, so a reflection call inside an IIFE is flagged.
        Expression::FunctionExpression(f) => {
            if let Some(body) = &f.body {
                collect_unsupported_stmts(&body.statements, out, state);
            }
        }
        Expression::AssignmentExpression(a) => {
            // Recurse the lvalue too — `obj[Symbol.X] = v` / `Array.prototype[k]
            // = v` bury reflection in the assignment target.
            collect_assignment_target(&a.left, out, state);
            collect_expr(&a.right, out, state);
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    collect_expr(e, out, state);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    collect_expr(&op.value, out, state);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect_expr(e, out, state);
            }
        }
        Expression::ParenthesizedExpression(p) => collect_expr(&p.expression, out, state),
        Expression::TSNonNullExpression(nn) => collect_expr(&nn.expression, out, state),
        Expression::StaticMemberExpression(sm) => {
            // `e.constructor.name` / `e.constructor.message` — the ES
            // error-class idiom on a `catch (e)` binding, lowered to the
            // `DsError`'s `name`/`message` field. The inner `e.constructor` is
            // otherwise `.constructor` reflection (Reject), so short-circuit
            // it: don't recurse (a non-`DsError` receiver surfaces honestly as
            // a cargo error — `x.constructor` has no field).
            let is_error_prop_chain = (sm.property.name.as_str() == "name"
                || sm.property.name.as_str() == "message")
                && matches!(
                    &sm.object,
                    Expression::StaticMemberExpression(inner)
                        if inner.property.name.as_str() == "constructor",
                );
            if !is_error_prop_chain && !is_static_value_read(expr) {
                collect_expr(&sm.object, out, state);
            }
        }
        Expression::ComputedMemberExpression(cm) => {
            collect_expr(&cm.object, out, state);
            collect_expr(&cm.expression, out, state);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                collect_expr(e, out, state);
            }
        }
        Expression::ChainExpression(c) => {
            // Optional chaining the translator can lower statically is a single
            // `identifier?.field` (member.rs::chain_expr). Anything else — an
            // optional call `?.method()`, an optional computed `?.[k]`, or a
            // nested `a?.b?.c` — has no static lowering (it emits `todo!()` or a
            // mistyped expression), so the function degrades to the engine.
            if chain_needs_engine(c) {
                out.push(err(
                    "optional chaining beyond a single `?.field` (a `?.method()` call, \
                     `?.[k]`, or a nested `a?.b?.c`) has no static lowering — the function \
                     runs under the engine",
                    c.span,
                ));
            } else if let ChainElement::StaticMemberExpression(sm) = &c.expression {
                // `id?.field` — recurse the base so a construct in it still
                // surfaces (the field name is not an expression).
                collect_expr(&sm.object, out, state);
            }
        }
        _ => {}
    }
}

/// Whether a `ChainExpression` is beyond the single `identifier?.field` form
/// [`super::expressions::member::chain_expr`] lowers statically, and so degrades
/// to the engine. A single optional static member on a plain identifier is the
/// only handled shape; an optional call, an optional computed access, a nested
/// chain, or a non-identifier base all need the engine.
fn chain_needs_engine(c: &ChainExpression) -> bool {
    match &c.expression {
        ChainElement::StaticMemberExpression(sm) => !matches!(sm.object, Expression::Identifier(_)),
        _ => true,
    }
}

fn err(message: impl Into<Cow<'static, str>>, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::error(message).with_label(span)
}

/// The global names DashScript models only as a static-call/new receiver or
/// type annotation — never as a first-class value. Delegates to the canonical
/// list in [`super::globals`] so the translator's dispatch and this lint share
/// one source of truth (no duplicated name list to drift).
fn is_global_object_name(name: &str) -> bool {
    super::globals::is_static_only_global(name)
}

/// True when `expr` is a global-object name in a call/new receiver position —
/// either a static member (`Math.floor`, `Array.isArray`) or a bare reference
/// (`new Map()`). Used to skip recursing the callee so the receiver is not
/// mistaken for a value reference.
fn is_global_object_callee(expr: &Expression) -> bool {
    // A `Global.X(…)` static call (`Math.floor(x)`) takes the global name only
    // as the call's receiver — `Global` is not a value reference. A two-level
    // `Global.Type.method(…)` (`Temporal.PlainDate.from(s)`) is the same shape:
    // `Global` is the member-chain root, not a value. Walk the chain down to
    // its root Identifier; a `prototype` segment anywhere in the chain marks a
    // reflection borrow (`Object.prototype.trim.call(…)`) — that stays visible
    // so the reflection rule flags it, rather than being muted as a static call
    // (which would let `Object.prototype.X(y)` slip through to a cargo partial).
    fn rooted_at_global(mut e: &Expression) -> bool {
        loop {
            match e {
                Expression::ParenthesizedExpression(p) => e = &p.expression,
                Expression::StaticMemberExpression(sm) => {
                    if sm.property.name.as_str() == "prototype" {
                        return false;
                    }
                    e = &sm.object;
                }
                Expression::Identifier(id) => {
                    // A `Static`-only global (`Math`, `Array`) or an `Engine`-
                    // value global as a chain root (`Temporal.PlainDate.from`)
                    // is the call's receiver, not a value reference — only its
                    // bare form is. Accept either as the root so the receiver
                    // is not recursed into (a bare `Temporal` value reference
                    // is still flagged by the Identifier rule below).
                    return is_global_object_name(id.name.as_str())
                        || super::globals::is_engine_value_global(id.name.as_str());
                }
                _ => return false,
            }
        }
    }
    match expr {
        Expression::StaticMemberExpression(_) => rooted_at_global(expr),
        Expression::Identifier(id) => is_global_object_name(id.name.as_str()),
        _ => false,
    }
}

/// Mapped static constants on `Math`/`Number` that may be read as values
/// (`Math.PI`, `Number.MAX_VALUE`, …). A `<Global>.<prop>` access where `prop`
/// is one of these (or `prototype`) is a static-value read, not a reflection.
const STATIC_VALUE_PROPS: &[&str] = &[
    "PI",
    "E",
    "LN2",
    "LN10",
    "LOG2E",
    "LOG10E",
    "SQRT2",
    "SQRT1_2",
    "MAX_VALUE",
    "MIN_VALUE",
    "EPSILON",
    "MAX_SAFE_INTEGER",
    "MIN_SAFE_INTEGER",
    "POSITIVE_INFINITY",
    "NEGATIVE_INFINITY",
    "NaN",
];

/// True when `expr` is a mapped static-value read — `<Global>.prototype` or
/// `<Global>.<staticConstant>` (`Math.PI`, `Number.MAX_VALUE`,
/// `Array.prototype`). These take a global receiver but are not value
/// references to it.
fn is_static_value_read(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(sm) if {
            let p = sm.property.name.as_str();
            (p == "prototype" || STATIC_VALUE_PROPS.contains(&p))
                && is_global_object_receiver(&sm.object)
        }
    )
}

/// True when `expr` is a global-object value a no-op static method may take
/// (and ignore) as an argument: a bare global name (`JSON`), `<Global>.
/// prototype`, or a mapped static constant (`Math.PI`). A method reference
/// (`Object.create`) or arity (`Math.floor.length`) is NOT matched — it stays
/// visible so [`collect_expr`] reaches the global name and flags it.
fn is_global_object_value(expr: &Expression) -> bool {
    match expr {
        Expression::Identifier(id) => is_global_object_name(id.name.as_str()),
        _ => is_static_value_read(expr),
    }
}

/// True when `expr` is a bare global receiver name (`Math`, `Number`) — the
/// root a static-member chain is read from. Delegates to the canonical list in
/// [`super::globals`] so the translator's dispatch and this lint agree.
fn is_global_object_receiver(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Identifier(id) if super::globals::is_global_receiver(id.name.as_str())
    )
}

/// True when `callee` is a prototype-method borrow whose callee should not be
/// recursed (so the `<builtin>.prototype.<method>` reflection rule does not
/// flag a legitimate borrow).
///
/// The `<Builtin>.prototype.<method>.call` shape match is delegated to the
/// translator's own [`super::super::expressions::call::prototype_method_call`]
/// — the single structural matcher, so check.rs and the translator cannot drift
/// on the AST shape (the bug that made the prior local matcher miss a layer).
///
/// Only `String` prototype borrows (`String.prototype.X.call(s, …)`) whitelist:
/// the static lowering (`str::X`) compiles for the common `&str`/`String`
/// receivers. `Array` prototype borrows (`Array.prototype.X.call(arr-like, …)`)
/// are not whitelisted — test262 drives them with non-`Vec` array-like
/// receivers the static `array_method_on` lowering cannot compile (0/790 in
/// test262), so flagging the `<builtin>.prototype.<method>` reflection routes
/// the enclosing function to the engine (which ships the builtin verbatim)
/// instead of a guaranteed-static `partial`. `check` and the engine detector
/// agree on this. Only `.call` is mapped; `.apply`/`.bind` fall through.
fn is_borrow_call(callee: &Expression) -> bool {
    matches!(
        super::expressions::call::prototype_method_call(callee),
        Some(("String", _))
    )
}
