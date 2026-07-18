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

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    AssignmentTarget, BinaryOperator, CallExpression, Expression, ForStatementInit,
    ObjectPropertyKind, Statement, UnaryOperator,
};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{SourceType, Span};

use super::name_table;
use super::{functions, registry};

/// Check `.ds` source for translatability. Returns syntax errors from
/// `oxc_parser` plus one diagnostic per construct the translator cannot map —
/// both unmapped top-level statements and low-compatibility constructs buried
/// inside a function body. An empty result means the file lowers to valid
/// Rust (as far as DashScript can tell — `cargo check` is still the final
/// arbiter).
pub(super) fn check(source: &str) -> Vec<OxcDiagnostic> {
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
    let registry = registry::build_registry(&program.body);
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
            collect_expr(&w.test, out);
            collect_unsupported(&w.body, out);
        }
        Statement::DoWhileStatement(dw) => {
            collect_unsupported(&dw.body, out);
            collect_expr(&dw.test, out);
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
                collect_expr(test, out);
            }
            if let Some(update) = &f.update {
                collect_expr(update, out);
            }
            collect_unsupported(&f.body, out);
        }
        Statement::ForOfStatement(fo) => collect_unsupported(&fo.body, out),
        Statement::ForInStatement(fi) => collect_unsupported(&fi.body, out),
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
            collect_expr(&c.callee, out);
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_expr(e, out);
                }
            }
        }
        // `new X(…)` — recurse the constructor and args so `new Proxy(…)` /
        // `new Symbol(…)` trip the identifier rule.
        Expression::NewExpression(n) => {
            collect_expr(&n.callee, out);
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
        Expression::StaticMemberExpression(sm) => collect_expr(&sm.object, out),
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
            _ => {}
        },
        Expression::CallExpression(c) => unsupported_call(c, out),
        // `.constructor` — prototype reflection.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "constructor" => {
            out.push(err("`.constructor` reflection is unsupported", sm.span));
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
    }
}

fn err(message: impl Into<Cow<'static, str>>, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::error(message).with_label(span)
}
