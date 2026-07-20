//! `.ds` translatability check — the middle layer of the three-layer
//! correctness chain (structure → translatability → `cargo check`).
//!
//! It reuses the translator's own mapping as the single source of truth: any
//! top-level statement [`super::functions::translate_statement`] cannot lower is
//! reported as a diagnostic, alongside the syntax errors `oxc_parser` already
//! surfaced. This answers "can this `.ds` become valid Rust?" — which
//! eslint-style rules cannot express, and which `oxc_linter` (not on crates.io)
//! is therefore not used for.
//!
//! A second pass walks the function body for **low-compatibility constructs**
//! ([`collect_unsupported`]) — ECMAScript dynamic/reflection features
//! (`instanceof`, `Symbol`/`Proxy`/`Reflect`, prototype reflection, `eval`,
//! `delete`, `arguments`) that have no Rust mapping. The translator would
//! otherwise lower them to broken Rust that fails `cargo check` (reported as
//! `partial`); flagging them here reports them honestly as `unsupported`, so
//! the conformance matrix reflects what DashScript can actually express rather
//! than what merely parses.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, AssignmentTarget, BinaryOperator, BindingPattern, CallExpression, Expression,
    ForStatementInit, ObjectPropertyKind, PropertyKind, Statement, UnaryOperator,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};

use super::name_table;
use super::{functions, registry};

// The set of prototype borrows `is_borrow_call` whitelists depends on the
// caller (`check` vs the engine detector). Rather than thread a `for_engine`
// bool through every recursive `collect_*` call (~30 sites), a thread-local
// flag carries it: `program_uses_engine` sets it for the duration of its walk
// (`EngineScope` resets it on drop — even on panic), `check` leaves it at the
// default `false`. Per-thread, so the conformance harness's parallel workers
// each carry their own.
thread_local! {
    static FOR_ENGINE: Cell<bool> = const { Cell::new(false) };
    /// True while `collect_unsupported` walks the body of a loop — a
    /// `re.exec(…)` there needs the engine, because regress is stateless and
    /// would re-find the same match every iteration (an infinite loop, killed
    /// only by the harness timeout). Set by `collect_loop_body`.
    static IN_LOOP: Cell<bool> = const { Cell::new(false) };
    /// Names of variables in the current `check` walk whose initializer is a
    /// plainly non-string literal (number/boolean/object/array). A later
    /// `.test(x)` / `.exec(x)` on one routes to the engine: ES coerces the
    /// argument via ToString, which regress (taking `&str`) cannot express;
    /// without this the translator emits `x.as_str()` and fails cargo check
    /// (E0599). Populated by `collect_unsupported`'s `VariableDeclaration`
    /// arm; cleared at each `check` / `program_uses_engine` entry so the set
    /// never leaks across programs on the same thread.
    static NON_STRING_VARS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
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

/// Check `.ds` source for translatability. Returns syntax errors from
/// `oxc_parser` plus one diagnostic per construct the translator cannot map —
/// both unmapped top-level statements and low-compatibility constructs buried
/// inside a function body. An empty result means the file lowers to valid
/// Rust (as far as DashScript can tell — `cargo check` is still the final
/// arbiter).
pub(super) fn check(source: &str) -> Vec<OxcDiagnostic> {
    // Reset the non-string-var set for this program (a prior `check` on the
    // same thread must not leak bindings forward — see [`NON_STRING_VARS`]).
    NON_STRING_VARS.with(|s| s.borrow_mut().clear());
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
    for stmt in &program.body {
        if functions::translate_statement(stmt, &registry, &names).is_empty() {
            diagnostics.push(unmapped_top_level(stmt));
        }
        // Low-compatibility constructs inside the body — see
        // [`collect_unsupported`].
        collect_unsupported(stmt, &mut diagnostics);
    }
    diagnostics
}

/// True when the program body contains any ES dynamic/reflection construct the
/// static translator cannot lower — the same walk as [`collect_unsupported`],
/// returning whether any construct was found (not the diagnostics). The
/// translator routes such a program through the embedded QuickJS engine
/// (`RuntimeDeps::needs_engine`) instead of static lowering, so a fixture that
/// uses `Object.defineProperty`/`Reflect.*`/`Symbol`/`instanceof`/… runs
/// correctly rather than failing `cargo check`.
///
/// Mirrors the unsupported-construct detection so the translator and `check`
/// agree on what the engine path covers — no second list to drift. Parse
/// errors are out of scope (a parse failure is fatal before this is called);
/// unmapped top-level statements are too (top-level hoisting is a separate
/// translator path, not an engine concern).
pub(super) fn program_uses_engine(program: &oxc_ast::ast::Program) -> bool {
    // For the duration of this walk, `is_borrow_call` whitelists every prototype
    // borrow the translator *attempts* (String + Array), so a borrow the
    // translator can lower is not needlessly stolen by the engine. The scope
    // guard resets the flag on drop — even on panic.
    FOR_ENGINE.with(|c| c.set(true));
    let _scope = EngineScope;
    // Reset the non-string-var set for this walk — see `check`.
    NON_STRING_VARS.with(|s| s.borrow_mut().clear());
    let mut diags = Vec::new();
    for stmt in &program.body {
        collect_unsupported(stmt, &mut diags);
    }
    !diags.is_empty()
}

/// A human message + span for a top-level statement the translator skips.
fn unmapped_top_level(stmt: &Statement) -> OxcDiagnostic {
    match stmt {
        Statement::ImportDeclaration(s) => err("module `import` is not supported yet", s.span),
        Statement::ExportNamedDeclaration(s) => err("module `export` is not supported yet", s.span),
        Statement::ExportDefaultDeclaration(s) => {
            err("module `export default` is not supported yet", s.span)
        }
        Statement::ExportAllDeclaration(s) => err("module `export *` is not supported yet", s.span),
        Statement::TSEnumDeclaration(s) => err(
            "TypeScript `enum` is not supported (use a union type instead)",
            s.span,
        ),
        Statement::ExpressionStatement(s) => err(
            "a top-level expression is not allowed — only function/interface/type \
             declarations may sit at module scope",
            s.span,
        ),
        Statement::VariableDeclaration(s) => err(
            "a top-level variable declaration is not allowed — move it into a function",
            s.span,
        ),
        _ => OxcDiagnostic::error("this top-level statement cannot be translated to Rust"),
    }
}

/// Walk a statement (and every expression nested inside it) collecting one
/// diagnostic per low-compatibility construct — see [`unsupported_pattern`].
/// Recurses through every statement/expression kind the translator itself
/// walks (mirroring `analysis::walk_stmt`), so a construct buried in a loop,
/// branch, or callback is still surfaced. Unfamiliar kinds fall through
/// silently (a missed construct only means it stays `partial`, not a false
/// `unsupported`).
fn collect_unsupported(stmt: &Statement, out: &mut Vec<OxcDiagnostic>) {
    match stmt {
        Statement::FunctionDeclaration(f) => {
            if let Some(body) = &f.body {
                for s in &body.statements {
                    // A nested `function` declaration (the test262 `callbackfn`
                    // convention) has no Rust mapping — a Rust `fn` item cannot
                    // sit inside another fn body in a way the translator lowers,
                    // so the declaration is dropped and the call site then fails
                    // `cargo check` (E0425 partial). Flag it here so it is
                    // reported as `unsupported` rather than as a partial.
                    if let Statement::FunctionDeclaration(nested) = s {
                        out.push(err(
                            "nested function declaration is unsupported — move it to \
                             module scope, or use an arrow function for a callback",
                            nested.span,
                        ));
                    }
                    collect_unsupported(s, out);
                }
            }
        }
        Statement::BlockStatement(b) => collect_unsupported_stmts(&b.body, out),
        // `try { … } catch (e) { … }` — recurse the try block, the handler
        // body, and the optional `finally`, so a construct inside the handler
        // (`e.constructor.name`) or the try body is still surfaced.
        Statement::TryStatement(t) => {
            collect_unsupported_stmts(&t.block.body, out);
            if let Some(handler) = &t.handler {
                collect_unsupported_stmts(&handler.body.body, out);
            }
            if let Some(fin) = &t.finalizer {
                collect_unsupported_stmts(&fin.body, out);
            }
        }
        Statement::ExpressionStatement(es) => collect_expr(&es.expression, out),
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    // A variable bound to a plainly non-string literal
                    // (number/boolean/object/array) — recorded so a later
                    // `.test(x)` / `.exec(x)` on it routes to the engine: ES
                    // coerces the argument via ToString, which regress (taking
                    // `&str`) cannot express. A function-call or identifier
                    // initializer may still yield a string, so it is left
                    // unrecorded (no false engine route).
                    if matches!(
                        init,
                        Expression::NumericLiteral(_)
                            | Expression::BooleanLiteral(_)
                            | Expression::ObjectExpression(_)
                            | Expression::ArrayExpression(_)
                    ) {
                        if let BindingPattern::BindingIdentifier(id) = &d.id {
                            NON_STRING_VARS
                                .with(|s| s.borrow_mut().insert(id.name.as_str().to_string()));
                        }
                    }
                    collect_expr(init, out);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_expr(&if_stmt.test, out);
            collect_unsupported(&if_stmt.consequent, out);
            if let Some(alt) = &if_stmt.alternate {
                collect_unsupported(alt, out);
            }
        }
        Statement::WhileStatement(w) => {
            collect_loop_expr(&w.test, out);
            collect_loop_body(&w.body, out);
        }
        Statement::DoWhileStatement(dw) => {
            collect_loop_body(&dw.body, out);
            collect_loop_expr(&dw.test, out);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    if let Some(i) = &d.init {
                        collect_expr(i, out);
                    }
                }
            }
            if let Some(test) = &f.test {
                collect_loop_expr(test, out);
            }
            if let Some(update) = &f.update {
                collect_loop_expr(update, out);
            }
            collect_loop_body(&f.body, out);
        }
        Statement::ForOfStatement(fo) => collect_loop_body(&fo.body, out),
        Statement::ForInStatement(fi) => collect_loop_body(&fi.body, out),
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                collect_expr(arg, out);
            }
        }
        Statement::ThrowStatement(t) => collect_expr(&t.argument, out),
        Statement::SwitchStatement(sw) => {
            collect_expr(&sw.discriminant, out);
            for c in &sw.cases {
                for s in &c.consequent {
                    collect_unsupported(s, out);
                }
            }
        }
        _ => {}
    }
}

/// Walk an assignment's left-hand target so a reflection nested in the lvalue
/// is surfaced — `obj[Symbol.X] = v` (the index holds a `Symbol` reference),
/// `Array.prototype[k] = v`, or `Array.prototype.foo = v` (mutating a builtin's
/// prototype). The receiver of a member target is recursed too, so a
/// reflection buried there is not missed. A plain `xs[i] = v` / `obj.f = v`
/// adds nothing (no reflection), so legitimate mutation stays supported.
fn collect_assignment_target(target: &AssignmentTarget, out: &mut Vec<OxcDiagnostic>) {
    match target {
        AssignmentTarget::ComputedMemberExpression(cm) => {
            if is_prototype_member(&cm.object) {
                out.push(err("`prototype` mutation is unsupported", cm.span));
            }
            collect_expr(&cm.object, out);
            collect_expr(&cm.expression, out);
        }
        AssignmentTarget::StaticMemberExpression(sm) => {
            if is_prototype_member(&sm.object) {
                out.push(err("`prototype` mutation is unsupported", sm.span));
            }
            // `x.index = …` / `.input` / `.indices` / `.groups` — assigning an
            // ES match-result field. It is read-only on a real match result
            // (ES throws in strict mode), so an assignment is the test262
            // idiom of stamping the property onto a plain Array
            // (`["a"].index = 2`) — dynamic property mutation the static model
            // cannot express. (A user struct field named `index` is rare and
            // would surface honestly as unsupported, not silently mis-compile.)
            if matches!(
                sm.property.name.as_str(),
                "index" | "input" | "indices" | "groups"
            ) {
                out.push(err(
                    "match-result property assignment is unsupported",
                    sm.span,
                ));
            }
            // `<re>.lastIndex = …` (write) — same stateless-cursor reason as
            // the read arm in `unsupported_pattern`; route to the engine.
            if sm.property.name.as_str() == "lastIndex" {
                out.push(err(
                    "regex `.lastIndex` assignment needs the engine (regress is stateless)",
                    sm.span,
                ));
            }
            collect_expr(&sm.object, out);
        }
        _ => {}
    }
}

/// True when `expr` is `<X>.prototype` — accessing (then mutating) a builtin's
/// prototype, which DashScript's static model cannot express.
fn is_prototype_member(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "prototype"
    )
}

/// Walk a slice of statements — the shared spine of [`collect_unsupported`]'s
/// block-shaped arms (a BlockStatement, a function/arrow body, try/catch
/// bodies).
fn collect_unsupported_stmts(stmts: &[Statement], out: &mut Vec<OxcDiagnostic>) {
    for s in stmts {
        collect_unsupported(s, out);
    }
}

/// Walk a loop body with [`IN_LOOP`] set, so a `re.exec(…)` inside (the
/// test262 `do { m = re.exec(s); … } while (1)` idiom) routes to the engine —
/// regress is stateless, so the loop would re-find the same match every
/// iteration (an infinite loop the harness times out at 30s).
fn collect_loop_body(body: &Statement, out: &mut Vec<OxcDiagnostic>) {
    let prev = IN_LOOP.with(|c| c.replace(true));
    collect_unsupported(body, out);
    IN_LOOP.with(|c| c.set(prev));
}

/// Walk a loop's per-iteration expression (a `while`/`do-while` test, or a
/// `for` test/update) with [`IN_LOOP`] set, so a `re.exec(…)` in the condition
/// — `while (re.exec(s) !== null)` — routes to the engine like one in the body.
/// (A `for` init is walked normally: it runs once, so a single `.exec` there
/// stays on the regress path.)
fn collect_loop_expr(expr: &Expression, out: &mut Vec<OxcDiagnostic>) {
    let prev = IN_LOOP.with(|c| c.replace(true));
    collect_expr(expr, out);
    IN_LOOP.with(|c| c.set(prev));
}

/// Detect a low-compatibility pattern at `expr`, then recurse into its
/// children. A `typeof` operand is **not** recursed: `typeof` has its own
/// mapping (a global constructor → `"function"`), so `typeof Symbol`/
/// `typeof Proxy` must stay supported rather than tripping the identifier
/// rule.
fn collect_expr(expr: &Expression, out: &mut Vec<OxcDiagnostic>) {
    unsupported_pattern(expr, out);
    match expr {
        Expression::UnaryExpression(u) => {
            if !matches!(u.operator, UnaryOperator::Typeof) {
                collect_expr(&u.argument, out);
            }
        }
        Expression::BinaryExpression(b) => {
            collect_expr(&b.left, out);
            collect_expr(&b.right, out);
        }
        Expression::LogicalExpression(l) => {
            collect_expr(&l.left, out);
            collect_expr(&l.right, out);
        }
        Expression::ConditionalExpression(c) => {
            collect_expr(&c.test, out);
            collect_expr(&c.consequent, out);
            collect_expr(&c.alternate, out);
        }
        Expression::CallExpression(c) => {
            // A global-object static call (`Math.floor(x)`, `Array.isArray(x)`,
            // `Object.keys(m)`, `JSON.parse(s)`) takes the global name only as
            // the call's receiver — not as a value reference. Don't recurse the
            // callee (its receiver would otherwise trip the identifier rule
            // below); recurse the arguments. Reflection methods
            // (`Object.defineProperty`) are caught separately by `unsupported_call`.
            if !is_global_object_callee(&c.callee) && !is_borrow_call(&c.callee) {
                collect_expr(&c.callee, out);
            }
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    // A global-object value as an argument
                    // (`Object.isExtensible(JSON)`, `Object.isExtensible(Array.prototype)`)
                    // is often an ignored param of a no-op method — the static
                    // call above already resolved it — so skip the value-reference
                    // rule on it; these stay supported. Other args are scanned.
                    if !is_global_object_value(e) {
                        collect_expr(e, out);
                    }
                }
            }
        }
        // `new X(…)` — recurse the constructor and args so `new Proxy(…)` /
        // `new Symbol(…)` trip the identifier rule. A global-object constructor
        // (`new Map()`, `new Set()`) is mapped, so its receiver is skipped.
        Expression::NewExpression(n) => {
            if !is_global_object_callee(&n.callee) {
                collect_expr(&n.callee, out);
            }
            for arg in &n.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_expr(e, out);
                }
            }
        }
        // `(x) => { … }` / `(x) => e` — recurse the arrow body so a construct
        // buried in a callback (`xs.forEach(x => x instanceof B)`) is surfaced.
        // oxc wraps even a concise body as a FunctionBody whose single statement
        // is an ExpressionStatement.
        Expression::ArrowFunctionExpression(a) => {
            collect_unsupported_stmts(&a.body.statements, out);
        }
        // `(function () { … })()` — a function expression's body is walked
        // too, so a reflection call inside an IIFE is flagged.
        Expression::FunctionExpression(f) => {
            if let Some(body) = &f.body {
                collect_unsupported_stmts(&body.statements, out);
            }
        }
        Expression::AssignmentExpression(a) => {
            // Recurse the lvalue too — `obj[Symbol.X] = v` / `Array.prototype[k]
            // = v` bury reflection in the assignment target.
            collect_assignment_target(&a.left, out);
            collect_expr(&a.right, out);
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    collect_expr(e, out);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    collect_expr(&op.value, out);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect_expr(e, out);
            }
        }
        Expression::ParenthesizedExpression(p) => collect_expr(&p.expression, out),
        Expression::TSNonNullExpression(nn) => collect_expr(&nn.expression, out),
        Expression::StaticMemberExpression(sm) => {
            // A mapped static read (`Math.PI`, `Number.MAX_VALUE`,
            // `Array.prototype`) takes a global receiver but is not a value
            // reference to it — don't recurse (it would trip the identifier
            // rule). A method name or arity on a global receiver
            // (`Object.create`, `Math.floor.length`) is reflection: recurse so
            // the global name is reached and flagged. Arity `.length` is also
            // caught directly by `unsupported_pattern`.
            if !is_static_value_read(expr) {
                collect_expr(&sm.object, out);
            }
        }
        Expression::ComputedMemberExpression(cm) => {
            collect_expr(&cm.object, out);
            collect_expr(&cm.expression, out);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                collect_expr(e, out);
            }
        }
        _ => {}
    }
}

/// A single low-compatibility construct at `expr` → one diagnostic. These are
/// ECMAScript dynamic/reflection features DashScript's static TS→Rust mapping
/// cannot express; flagging them here (rather than letting the translator
/// emit broken Rust) is what makes the conformance matrix honest about
/// coverage. Reflection calls are delegated to [`unsupported_call`].
fn unsupported_pattern(expr: &Expression, out: &mut Vec<OxcDiagnostic>) {
    match expr {
        // `x instanceof T` — a runtime type check with no static equivalent.
        Expression::BinaryExpression(b) if matches!(b.operator, BinaryOperator::Instanceof) => {
            out.push(err(
                "`instanceof` has no DashScript mapping (static types; no runtime type check)",
                b.span,
            ));
        }
        // `delete x` — no Rust analogue.
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Delete) => {
            out.push(err("`delete` has no DashScript mapping", u.span));
        }
        // Reflection / metaprogramming globals, and `arguments`/`eval`.
        Expression::Identifier(id) => match id.name.as_str() {
            "Symbol" | "Proxy" | "WeakRef" | "FinalizationRegistry" => {
                out.push(err(
                    format!("`{}` (JS reflection) is unsupported", id.name),
                    id.span,
                ));
            }
            "arguments" => out.push(err("the `arguments` object is unsupported", id.span)),
            "eval" => out.push(err("`eval` is unsupported", id.span)),
            // The global object/constructor names DashScript models only as a
            // static-call/new receiver (`Array.isArray(x)`, `new Map()`) or a
            // type annotation (`Map<K,V>`) — never as a first-class value.
            // Referencing one as a value (`Array.isArray`, `Array.isArray.length`,
            // `var f = Object.keys`, `Math.prototype`) is reflection the static
            // TS→Rust mapping cannot express; without this the translator would
            // snake-case the name (`Array`→`array`) and emit broken Rust (E0425
            // `partial`). The typeof/global-call paths short-circuit before
            // reaching here, so legitimate uses stay supported.
            name if is_global_object_name(name) => out.push(err(
                format!(
                    "`{name}` as a value is unsupported (use it only as a static-call/new \
                     receiver or type annotation)"
                ),
                id.span,
            )),
            _ => {}
        },
        Expression::CallExpression(c) => unsupported_call(c, out),
        // `.constructor` — prototype reflection.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "constructor" => {
            out.push(err("`.constructor` reflection is unsupported", sm.span));
        }
        // `<re>.lastIndex` (read) — the ES regex stateful cursor. regress is
        // stateless (no `lastIndex` field, so this would be E0609), so route to
        // the engine, whose exec/test advance `lastIndex` like ES.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "lastIndex" => {
            out.push(err(
                "regex `.lastIndex` needs the engine (regress is stateless)",
                sm.span,
            ));
        }
        // `<Global>.<method>.length` — function arity reflection
        // (`Math.floor.length`, `Object.create.length`). The static member read
        // itself is mapped, but reading its `.length` (formal parameter count)
        // is reflection the static mapping cannot express.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "length" && is_global_method_chain(&sm.object) =>
        {
            out.push(err(
                "`<builtin>.<method>.length` arity reflection is unsupported",
                sm.span,
            ));
        }
        // `<Global>.prototype.<method>` — a prototype method read as a value
        // (`Object.prototype.toString`, `Array.prototype.concat`). The
        // prototype itself is a mapped static read, but a method hanging off
        // it (without a `.call`/`.apply`/`.bind` invocation) is reflection.
        // Those borrows are skipped in collect_expr's CallExpression arm.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() != "prototype"
                && matches!(
                    &sm.object,
                    Expression::StaticMemberExpression(outer)
                        if outer.property.name.as_str() == "prototype"
                            && is_global_object_receiver(&outer.object)
                ) =>
        {
            out.push(err(
                "`<builtin>.prototype.<method>` reflection is unsupported",
                sm.span,
            ));
        }
        // `{ get x() { … } }` / `{ set x(v) { … } }` — accessor properties have
        // no Rust struct/HashMap analogue (a field is a plain value, not a
        // computed property), and a getter's side effect of adding an own key
        // during enumeration has no static lowering.
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    if matches!(op.kind, PropertyKind::Get | PropertyKind::Set) {
                        out.push(err(
                            "object accessor properties (get/set) are unsupported",
                            op.span,
                        ));
                    }
                }
            }
        }
        // `123n` — BigInt literals (DashScript numbers are `f64`/`i64`).
        Expression::BigIntLiteral(b) => {
            out.push(err("`BigInt` literals are unsupported", b.span));
        }
        _ => {}
    }
}

/// A reflection call — `Object.defineProperty`/`getOwnPropertyDescriptor`/…,
/// `Reflect.*`, or an instance prototype reflection `x.hasOwnProperty`/
/// `propertyIsEnumerable`/`isPrototypeOf`. `Object.keys`/`values`/`entries`/
/// `is`/`freeze`/… are mapped and intentionally absent from the reflection
/// list, so they must not trip this.
fn unsupported_call(c: &CallExpression, out: &mut Vec<OxcDiagnostic>) {
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return;
    };
    let prop = sm.property.name.as_str();
    // `<re>.exec(…)` inside a loop body — regress is stateless, so the loop
    // would re-find the same match every iteration (an infinite loop, killed
    // only by the harness timeout). The engine (rquickjs) advances `lastIndex`
    // like ES, so a looped exec routes there. (`/pat/.exec(s)` once, outside a
    // loop, stays on the regress path — it is a single `find`.)
    if prop == "exec" && IN_LOOP.with(|c| c.get()) {
        out.push(err(
            "regex `.exec` inside a loop needs the engine (regress is stateless)",
            sm.span,
        ));
    }
    // `.test(x)` / `.exec(x)` where x is plainly not a string — ES coerces
    // the argument via ToString, but regress takes `&str`, so a non-string
    // argument lowers to `x.as_str()` and fails cargo check (E0599). Route
    // to the engine, whose ToString matches ES. (The looped-exec case above
    // already routes; this catches the once-per-call non-string argument.)
    if matches!(prop, "test" | "exec") && regex_arg_needs_engine(&c.arguments) {
        out.push(err(
            "regex `.test`/`.exec` on a non-string needs the engine (ES ToString coercion)",
            sm.span,
        ));
    }
    // `.indexOf(x)` / `.lastIndexOf(x)` / `.includes(x)` where x is plainly not
    // a number — ES uses SameValueZero (indexOf/lastIndexOf) / strict equality
    // (includes), which distinguish `true` from `1` and `"1"` from `1`.
    // DashScript's `Vec<f64>` search assumes a numeric needle, so a non-number
    // needle lowers to `y == bool` (E0277) or `Vec::contains(&bool)` (E0308).
    // Route to the engine, whose element comparison matches ES. (A numeric needle
    // stays on the mapped path; a non-number *literal* or `undefined` triggers.)
    if matches!(prop, "indexOf" | "lastIndexOf" | "includes")
        && array_search_arg_needs_engine(&c.arguments)
    {
        out.push(err(
            "`.indexOf`/`.lastIndexOf`/`.includes` on a non-number needs the engine (ES SameValueZero/strict equality)",
            sm.span,
        ));
    }
    // `s.toLocaleUpperCase(locale)` / `toLocaleLowerCase(locale)` — locale-aware
    // casing. DashScript has no ICU locale table, so an explicit locale cannot
    // be honored; the locale-less form lowers to the default casing (see
    // `map_method`), but a locale argument is reported honestly as unsupported
    // rather than silently dropped (which would be wrong for tr/el/lt/…).
    if matches!(prop, "toLocaleUpperCase" | "toLocaleLowerCase") && !c.arguments.is_empty() {
        out.push(err(
            "locale-aware `toLocale*` with a locale argument is unsupported",
            sm.span,
        ));
        return;
    }
    // Instance prototype reflection methods.
    if matches!(
        prop,
        "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf"
    ) {
        out.push(err(
            format!("`{prop}` (prototype reflection) is unsupported"),
            sm.span,
        ));
        return;
    }
    if let Expression::Identifier(obj) = &sm.object {
        // The `Object` static reflection surface — the names DashScript does
        // NOT map. Everything else on `Object` that test262 probes (keys/
        // values/entries/is/freeze/seal/assign/…) has a mapping.
        let is_object_reflection = matches!(
            prop,
            "defineProperty"
                | "getOwnPropertyDescriptor"
                | "defineProperties"
                | "create"
                | "getPrototypeOf"
                | "setPrototypeOf"
                | "getOwnPropertyDescriptors"
                | "getOwnPropertySymbols"
        );
        if obj.name.as_str() == "Object" && is_object_reflection {
            out.push(err(
                format!("`Object.{prop}` reflection is unsupported"),
                sm.span,
            ));
        }
        // The entire `Reflect` namespace is reflection.
        if obj.name.as_str() == "Reflect" {
            out.push(err("`Reflect` is unsupported", sm.span));
        }
        // `String.raw` — the tagged-template runtime form. DashScript has no
        // tagged-template model, and `String.raw` builds from a `{ raw }`
        // template object the static mapping cannot express. Without this the
        // translator snake-cases `String` → `string` and emits broken Rust
        // (E0425 `partial`).
        if obj.name.as_str() == "String" && prop == "raw" {
            out.push(err(
                "`String.raw` (tagged template) is unsupported",
                sm.span,
            ));
        }
    }
}

/// True when a regex method's first argument is plainly not a string — either
/// a non-string literal, or an identifier bound (in this walk) to one. See
/// [`NON_STRING_VARS`]. Regress takes `&str`, so such an argument would lower
/// to `x.as_str()` (E0599); the caller reports it so the fixture routes to the
/// engine, whose ES ToString coercion handles number/boolean/object/… .
fn regex_arg_needs_engine(args: &[Argument]) -> bool {
    let Some(arg) = args.first().and_then(|a| a.as_expression()) else {
        return false;
    };
    match arg {
        // A plainly non-string literal (number/boolean/object/array/null).
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::NullLiteral(_) => true,
        // `void <expr>` evaluates to `undefined` → ToString "undefined".
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Void) => true,
        Expression::Identifier(id) => {
            // `undefined` (the global) coerces the same way as `void 0`.
            if id.name.as_str() == "undefined" {
                return true;
            }
            // A variable bound (in this walk) to a non-string literal.
            NON_STRING_VARS.with(|s| s.borrow().contains(id.name.as_str()))
        }
        _ => false,
    }
}

/// True when an `.indexOf`/`.lastIndexOf`/`.includes` search element is plainly
/// not a number — a non-number literal, or `undefined`. ES uses SameValueZero
/// (indexOf/lastIndexOf) / strict equality (includes), which distinguish types,
/// but DashScript's `Vec<f64>` search assumes a numeric needle, so a non-number
/// would be a type error (E0277/E0308). The fixture routes to the engine, whose
/// element comparison matches ES. (Mirrors [`regex_arg_needs_engine`] for the
/// regex ToString case; this is the numeric complement.)
fn array_search_arg_needs_engine(args: &[Argument]) -> bool {
    let Some(arg) = args.first().and_then(|a| a.as_expression()) else {
        return false;
    };
    match arg {
        // A plainly non-number, non-string literal (boolean/object/array/null).
        // A string needle is intentionally NOT routed: `"abc".indexOf("b")` is
        // the normal string-search case, and only an array with a string needle
        // is the E0308 (rarer) — routing every `string.indexOf` through the
        // engine to catch it would regress the common path. bool/object/null
        // needles route for both receivers: array (type mismatch) and string
        // (ES ToString of bool/object differs from a naive cast).
        Expression::BooleanLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::NullLiteral(_) => true,
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Void) => true,
        Expression::Identifier(id) if id.name.as_str() == "undefined" => true,
        _ => false,
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
    match expr {
        Expression::StaticMemberExpression(sm) => matches!(
            &sm.object,
            Expression::Identifier(id) if is_global_object_name(id.name.as_str())
        ),
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

/// True when `expr` is a `<Global>.<method>` chain (`Math.floor`,
/// `Object.create`, `Number.isFinite`) — a static method read as a value,
/// e.g. to then take its `.length` (arity reflection).
fn is_global_method_chain(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(sm) if is_global_object_receiver(&sm.object)
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
/// Which builtins whitelist is caller-dependent (the [`FOR_ENGINE`] thread
/// local): `check` whitelists only `String` — the translator's `array_method_on`
/// lowers Array borrows too, but 0/790 test262 Array borrows compile (non-`Vec`
/// receivers), so `check` keeps them `unsupported` rather than `partial`
/// (honest binary). The engine detector whitelists `String` + `Array` — every
/// borrow the translator *attempts* — so the engine fallback (a last resort for
/// constructs with no lowering at all) does not steal a borrow the translator
/// can lower. Only `.call` is mapped; `.apply`/`.bind` fall through.
fn is_borrow_call(callee: &Expression) -> bool {
    let for_engine = FOR_ENGINE.with(|c| c.get());
    match super::expressions::call::prototype_method_call(callee) {
        Some(("String", _)) => true,
        Some(("Array", _)) => for_engine,
        _ => false,
    }
}
