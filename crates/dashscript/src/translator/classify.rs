//! Translatability classification — the translator's single source of truth
//! for what it can lower to Rust. Each AST node maps to [`Mapping`]: lowered
//! to idiomatic Rust, rejected outright (no engine fallback), or degradable to
//! the embedded QuickJS engine. `check` and `program_uses_engine` query this
//! rather than keeping a parallel rule tree — the drift that today lets a new
//! translator mapping not auto-relax a `check` rejection.
//!
//! A classification carries its own diagnostic message, so the rule and its
//! wording live in one place. The `check` walk supplies the span and turns a
//! non-`Mapped` verdict into an `OxcDiagnostic`; it does not re-derive the
//! verdict or the message.
//!
//! Context-dependent rules (a regex `.exec` inside a loop, a `.test`/`.exec`
//! on a non-string binding set elsewhere) read [`ClassifyCtx`], which the walk
//! builds. Context-free rules ignore it.

use std::borrow::Cow;
use std::collections::HashSet;

use oxc_ast::ast::{
    Argument, AssignmentTarget, BinaryOperator, CallExpression, Class, Expression, Function,
    ObjectPropertyKind, PropertyKind, UnaryOperator,
};

use super::globals::{is_engine_value_global, is_global_receiver, is_static_only_global};

/// How a single AST node lowers — the translator's translatability verdict,
/// carrying the diagnostic message for a non-mapped outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mapping {
    /// The translator lowers this node to idiomatic Rust.
    Mapped,
    /// No static lowering and no engine fallback — a hard `unsupported`
    /// (`instanceof`, `delete`, reflection globals, accessor properties,
    /// `BigInt` literal, `await`, prototype mutation, …). The message is the
    /// diagnostic wording.
    Reject(Cow<'static, str>),
    /// No static lowering, but the engine runs it verbatim (a `Function`
    /// value as a callee/argument, regex `.lastIndex`, `JSON.<method>` other
    /// than parse/stringify, a looped `.exec`, …). The planned per-function
    /// fallback routes just the enclosing function to the engine.
    DegradeEngine(Cow<'static, str>),
}

impl Mapping {
    /// True when the node lowers to idiomatic Rust (no diagnostic).
    #[allow(dead_code)] // a convenience query; the B6 per-function fallback uses it
    pub fn is_mapped(&self) -> bool {
        matches!(self, Mapping::Mapped)
    }
}

/// Traverse state a context-dependent classification reads — the bits the AST
/// walk tracks so a looped `re.exec` or a non-string regex argument routes to
/// the engine. The `check`/`program_uses_engine` walk builds and updates this;
/// classification only reads it.
#[derive(Debug, Clone, Copy)]
pub struct ClassifyCtx<'a> {
    /// True while classifying an expression inside a loop body or per-iteration
    /// condition — a `re.exec(…)` there needs the engine, because regress is
    /// stateless and would re-find the same match every iteration.
    pub in_loop: bool,
    /// Locals in the current walk whose initializer is a plainly non-string
    /// literal (number/boolean/object/array) — a `.test(x)`/`.exec(x)` on one
    /// needs the engine (ES coerces via ToString; regress takes `&str`).
    pub non_string_vars: &'a HashSet<String>,
}

/// Classify a single expression. See [`Mapping`] for the verdicts.
pub(super) fn classify_expr(expr: &Expression, ctx: &ClassifyCtx) -> Mapping {
    match expr {
        // TS type-layer wrappers and user parens carry no runtime meaning —
        // classify the inner expression.
        Expression::ParenthesizedExpression(p) => classify_expr(&p.expression, ctx),
        // `x as Record<…>` / `as { [k]: … }` — casting a value to a dynamic
        // record to use string-keyed indexing. The cast type has no static
        // Rust form (`unknown`/indexed/`Record<dyn>`/…), so the enclosing
        // function degrades to the engine rather than silently mis-lowering
        // the cast onto a struct (which is not string-indexable).
        Expression::TSAsExpression(a) => {
            if super::types::type_has_unmappable(&a.type_annotation) {
                return degrade("cast to a type with no static Rust form needs the engine");
            }
            classify_expr(&a.expression, ctx)
        }
        Expression::TSTypeAssertion(t) => {
            if super::types::type_has_unmappable(&t.type_annotation) {
                return degrade("type assertion with no static Rust form needs the engine");
            }
            classify_expr(&t.expression, ctx)
        }
        Expression::TSNonNullExpression(n) => classify_expr(&n.expression, ctx),

        // `x instanceof T` — a runtime type check with no static equivalent.
        Expression::BinaryExpression(b) if matches!(b.operator, BinaryOperator::Instanceof) => {
            reject("`instanceof` has no DashScript mapping (static types; no runtime type check)")
        }
        // `delete x` — no Rust analogue.
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Delete) => {
            reject("`delete` has no DashScript mapping")
        }
        // Reflection/metaprogramming globals, `arguments`, `eval`, and a
        // global-object name read as a first-class value (the translator models
        // these only as a static-call/new receiver or type annotation).
        Expression::Identifier(id) => match id.name.as_str() {
            "Symbol" | "Proxy" | "WeakRef" | "FinalizationRegistry" => {
                reject_owned(format!("`{}` (JS reflection) is unsupported", id.name))
            }
            "arguments" => reject("the `arguments` object is unsupported"),
            "eval" => reject("`eval` is unsupported"),
            name if is_engine_value_global(name) => degrade_owned(format!(
                "`{name}` has no static mapping — the function runs under the engine"
            )),
            name if is_static_only_global(name) => reject_owned(format!(
                "`{name}` as a value is unsupported (use it only as a static-call/new receiver or \
                 type annotation)"
            )),
            _ => Mapping::Mapped,
        },
        // A reflection call, a `Function`-value callee/argument, or a dynamic
        // method whose engine routing depends on the argument/loop context.
        Expression::CallExpression(c) => classify_call(c, ctx),
        // `.constructor` — prototype reflection.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "constructor" => {
            reject("`.constructor` reflection is unsupported")
        }
        // `<re>.lastIndex` — the ES regex stateful cursor; regress is
        // stateless, so route to the engine.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "lastIndex" => {
            degrade("regex `.lastIndex` needs the engine (regress is stateless)")
        }
        // `<Global>.<method>.length` — function arity reflection.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "length" && is_global_method_chain(&sm.object) =>
        {
            reject("`<builtin>.<method>.length` arity reflection is unsupported")
        }
        // `<Global>.prototype.<method>` — a prototype method read as a value.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() != "prototype"
                && matches!(
                    &sm.object,
                    Expression::StaticMemberExpression(outer)
                        if outer.property.name.as_str() == "prototype"
                            && is_global_object_receiver(&outer.object),
                ) =>
        {
            reject("`<builtin>.prototype.<method>` reflection is unsupported")
        }
        // `{ get x() { … } }` / `{ set x(v) { … } }` — accessor properties.
        Expression::ObjectExpression(o) => {
            if o.properties.iter().any(|p| {
                matches!(
                    p,
                    ObjectPropertyKind::ObjectProperty(op)
                        if matches!(op.kind, PropertyKind::Get | PropertyKind::Set)
                )
            }) {
                reject("object accessor properties (get/set) are unsupported")
            } else {
                Mapping::Mapped
            }
        }
        // `123n` — BigInt literals.
        Expression::BigIntLiteral(_) => reject("`BigInt` literals are unsupported"),
        // `await expr` — DashScript has no async runtime.
        Expression::AwaitExpression(_) => {
            reject("`await` is unsupported (DashScript has no async runtime)")
        }
        _ => Mapping::Mapped,
    }
}

/// Classify an assignment's left-hand target — `prototype` mutation, an ES
/// match-result field write, or a `<re>.lastIndex = …` write. The target's
/// object/expression children are classified separately by the walk.
pub(super) fn classify_assignment_target(target: &AssignmentTarget) -> Mapping {
    match target {
        AssignmentTarget::ComputedMemberExpression(cm) => {
            if is_prototype_member(&cm.object) {
                reject("`prototype` mutation is unsupported")
            } else {
                Mapping::Mapped
            }
        }
        AssignmentTarget::StaticMemberExpression(sm) => {
            if is_prototype_member(&sm.object) {
                return reject("`prototype` mutation is unsupported");
            }
            // `x.index = …` / `.input` / `.indices` / `.groups` — stamping an
            // ES match-result field onto a plain Array; read-only on a real
            // match result, so the assignment is dynamic mutation.
            if matches!(
                sm.property.name.as_str(),
                "index" | "input" | "indices" | "groups"
            ) {
                return reject("match-result property assignment is unsupported");
            }
            // `<re>.lastIndex = …` (write) — same stateless-cursor reason as
            // the read arm above; route to the engine.
            if sm.property.name.as_str() == "lastIndex" {
                return degrade(
                    "regex `.lastIndex` assignment needs the engine (regress is stateless)",
                );
            }
            Mapping::Mapped
        }
        _ => Mapping::Mapped,
    }
}

/// Classify a function declaration's signature — its parameter and return
/// type annotations. A signature that carries a type the static translator
/// cannot express (`unknown`, `Record<string, unknown>`, an indexed access,
/// …) cannot be statically typed: the param/return would be `_`, which cargo
/// check rejects in a signature. The function therefore degrades to the engine
/// — its body runs verbatim under QuickJS, and the untypable types marshal as
/// `serde_json::Value`. This is the type-driven half of degradation; the
/// AST-driven half (regex `.lastIndex`, a `Function` value, …) lives in
/// [`classify_expr`].
pub(in crate::translator) fn classify_function_signature(f: &Function) -> Mapping {
    let unmappable_param = f.params.items.iter().any(|p| {
        p.type_annotation
            .as_deref()
            .is_some_and(|ta| super::types::type_has_unmappable(&ta.type_annotation))
    });
    let unmappable_return = f
        .return_type
        .as_deref()
        .is_some_and(|rt| super::types::type_has_unmappable(&rt.type_annotation));
    if unmappable_param || unmappable_return {
        degrade_owned(
            "a parameter or return type has no static Rust type (`unknown`/indexed access/…) — \
             the function runs under the engine"
                .to_string(),
        )
    } else {
        Mapping::Mapped
    }
}

/// Classify a class declaration/expression. A class with a `super_class`
/// (`extends`) cannot lower statically — DashScript models composition, not
/// inheritance, so `class B extends A` reaches the static translator only as a
/// `compile_error!` (see `class::translate_class`). A `.js`/`.mjs`/`.cjs`
/// module whose class extends another (e.g. `class _A extends B`) must
/// therefore degrade wholesale to the engine, where QuickJS runs the real
/// prototype chain. A single-base class with only a constructor and methods
/// stays `Mapped` (the #130-132 lowering).
pub(in crate::translator) fn classify_class(class: &Class) -> Mapping {
    if class.super_class.is_some() {
        return degrade("class `extends` needs the engine (no static inheritance lowering)");
    }
    Mapping::Mapped
}

/// Classify a call expression: reflection methods reject; a `Function` value
/// as callee/argument, `JSON.<other>`, or a dynamic regex/search method
/// degrades to the engine.
fn classify_call(c: &CallExpression, ctx: &ClassifyCtx) -> Mapping {
    // A `function` expression as the callee (an IIFE) or as an argument (a
    // callback) has no static lowering — degrade to the engine.
    if is_function_expression(&c.callee)
        || c.arguments
            .iter()
            .any(|a| a.as_expression().is_some_and(is_function_expression))
    {
        return degrade(
            "a `function` expression as a callee (IIFE) or argument (callback) needs the engine \
             (no static lowering)",
        );
    }
    // Bare `assert(x)` — test262's shorthand for `assert.sameValue(x, true)`.
    // No static lowering yet; degrade so the engine's `assert.js` runs it.
    if let Expression::Identifier(id) = &c.callee {
        if id.name.as_str() == "assert" {
            return degrade("`assert(x)` needs the engine (test262 harness)");
        }
    }
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return Mapping::Mapped;
    };
    let prop = sm.property.name.as_str();
    // `<re>.exec(…)` inside a loop — regress is stateless, so the loop would
    // re-find the same match every iteration. The engine advances
    // `lastIndex` like ES.
    if prop == "exec" && ctx.in_loop {
        return degrade("regex `.exec` inside a loop needs the engine (regress is stateless)");
    }
    // `.test(x)` / `.exec(x)` where x is plainly not a string — ES coerces via
    // ToString, but regress takes `&str`, so the argument would fail cargo
    // check. The engine's ToString matches ES.
    if matches!(prop, "test" | "exec") && regex_arg_needs_engine(&c.arguments, ctx) {
        return degrade(
            "regex `.test`/`.exec` on a non-string needs the engine (ES ToString coercion)",
        );
    }
    // `.replace`/`.replaceAll` with a callback (regex-driven replacement) has
    // no static lowering — regress exposes no per-match hook the callback could
    // call, and the callback receives the match as a value (not `&str`). A
    // plain-string/Pattern replacement stays on the static path.
    if matches!(prop, "replace" | "replaceAll")
        && c.arguments.iter().any(|a| {
            a.as_expression().is_some_and(|e| {
                matches!(
                    e,
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                )
            })
        })
    {
        return degrade(
            "`.replace`/`.replaceAll` with a callback needs the engine (no static per-match \
             lowering)",
        );
    }
    // `.indexOf(x)` / `.lastIndexOf(x)` / `.includes(x)` where x is plainly not
    // a number — ES uses SameValueZero / strict equality, which distinguish
    // types; DashScript's `Vec<f64>` search assumes a numeric needle.
    if matches!(prop, "indexOf" | "lastIndexOf" | "includes")
        && array_search_arg_needs_engine(&c.arguments)
    {
        return degrade(
            "`.indexOf`/`.lastIndexOf`/`.includes` on a non-number needs the engine (ES \
             SameValueZero/strict equality)",
        );
    }
    // `s.toLocaleUpperCase(locale)` / `toLocaleLowerCase(locale)` — locale-aware
    // casing with an explicit locale the locale-less mapping cannot honor.
    if matches!(prop, "toLocaleUpperCase" | "toLocaleLowerCase") && !c.arguments.is_empty() {
        return reject("locale-aware `toLocale*` with a locale argument is unsupported");
    }
    // Instance prototype reflection methods.
    if matches!(
        prop,
        "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf"
    ) {
        return reject_owned(format!("`{prop}` (prototype reflection) is unsupported"));
    }
    if let Expression::Identifier(obj) = &sm.object {
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
            return reject_owned(format!("`Object.{prop}` reflection is unsupported"));
        }
        // The entire `Reflect` namespace is reflection.
        if obj.name.as_str() == "Reflect" {
            return reject("`Reflect` is unsupported");
        }
        // `String.raw` — the tagged-template runtime form.
        if obj.name.as_str() == "String" && prop == "raw" {
            return reject("`String.raw` (tagged template) is unsupported");
        }
        // `JSON.<method>` other than parse/stringify (e.g. rawJSON/isRawJSON) —
        // no static mapping, so degrade to the engine, whose JSON matches ES.
        if obj.name.as_str() == "JSON" && !matches!(prop, "parse" | "stringify") {
            return degrade_owned(format!(
                "`JSON.{prop}` has no static mapping (only parse/stringify)"
            ));
        }
        // `assert.<m>` — sameValue/notSameValue lower statically; the rest
        // degrades to the engine (assert.js/propertyHelper.js run natively).
        if obj.name.as_str() == "assert" {
            return classify_assert(prop, &c.arguments);
        }
    }
    Mapping::Mapped
}

/// `assert.<m>(…)` — test262's harness. `sameValue`/`notSameValue` on scalar
/// operands lower to a Rust SameValue check; everything else (`throws`,
/// `compareArray`, reflection helpers, or a composite operand whose ES
/// SameValue is reference identity) degrades to the engine, where `assert.js`
/// and `propertyHelper.js` run natively.
fn classify_assert(prop: &str, args: &[Argument]) -> Mapping {
    match prop {
        "sameValue" | "notSameValue" => {
            let composite = args.iter().take(2).any(|a| {
                matches!(
                    a,
                    Argument::ObjectExpression(_) | Argument::ArrayExpression(_)
                )
            });
            if composite {
                degrade_owned(format!(
                    "`assert.{prop}` on a composite needs the engine (ES reference SameValue)"
                ))
            } else {
                Mapping::Mapped
            }
        }
        // `throws`/`compareArray`/`verifyProperty`/… — the engine runs the
        // test262 harness (`assert.js`/`propertyHelper.js`/`compareArray.js`)
        // natively. `throws` gets a static form in a later batch.
        _ => degrade_owned(format!(
            "`assert.{prop}` needs the engine (test262 harness)"
        )),
    }
}

/// True when a regex method's first argument is plainly not a string — either
/// a non-string literal, `undefined`, or an identifier bound (in this walk) to
/// one. Regress takes `&str`, so such an argument would fail cargo check; the
/// engine's ES ToString coercion handles number/boolean/object/… .
fn regex_arg_needs_engine(args: &[Argument], ctx: &ClassifyCtx) -> bool {
    let Some(arg) = args.first().and_then(|a| a.as_expression()) else {
        return false;
    };
    match arg {
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::NullLiteral(_) => true,
        // `void <expr>` evaluates to `undefined` → ToString "undefined".
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Void) => true,
        Expression::Identifier(id) => {
            if id.name.as_str() == "undefined" {
                return true;
            }
            ctx.non_string_vars.contains(id.name.as_str())
        }
        _ => false,
    }
}

/// True when an `.indexOf`/`.lastIndexOf`/`.includes` search element is plainly
/// not a number — a non-number, non-string literal, or `undefined`. ES uses
/// SameValueZero / strict equality (which distinguish types); DashScript's
/// `Vec<f64>` search assumes a numeric needle. A string needle is intentionally
/// not routed (the common `string.indexOf` path stays mapped).
fn array_search_arg_needs_engine(args: &[Argument]) -> bool {
    let Some(arg) = args.first().and_then(|a| a.as_expression()) else {
        return false;
    };
    match arg {
        Expression::BooleanLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::NullLiteral(_) => true,
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Void) => true,
        Expression::Identifier(id) if id.name.as_str() == "undefined" => true,
        _ => false,
    }
}

/// True when `expr` is a `function` expression, unwrapping the paren / TS
/// wrappers oxc keeps around an IIFE callee or a typed callback.
fn is_function_expression(e: &Expression) -> bool {
    match e {
        Expression::FunctionExpression(_) => true,
        Expression::ParenthesizedExpression(p) => is_function_expression(&p.expression),
        Expression::TSAsExpression(a) => is_function_expression(&a.expression),
        Expression::TSTypeAssertion(t) => is_function_expression(&t.expression),
        Expression::TSNonNullExpression(n) => is_function_expression(&n.expression),
        _ => false,
    }
}

/// `<Global>.<method>` chain — a static method read as a value (arity prefix).
fn is_global_method_chain(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(sm) if is_global_object_receiver(&sm.object)
    )
}

/// A bare global receiver name (`Math`, `Number`, …) — the root a static-member
/// chain is read from.
fn is_global_object_receiver(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Identifier(id) if is_global_receiver(id.name.as_str())
    )
}

/// True when `expr` is `<X>.prototype` — accessing (then mutating) a builtin's
/// prototype, which DashScript's static model cannot express.
fn is_prototype_member(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "prototype"
    )
}

fn reject(msg: &'static str) -> Mapping {
    Mapping::Reject(Cow::Borrowed(msg))
}

fn reject_owned(msg: String) -> Mapping {
    Mapping::Reject(Cow::Owned(msg))
}

fn degrade(msg: &'static str) -> Mapping {
    Mapping::DegradeEngine(Cow::Borrowed(msg))
}

fn degrade_owned(msg: String) -> Mapping {
    Mapping::DegradeEngine(Cow::Owned(msg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_first_expr(src: &str) -> Mapping {
        classify_first_expr_ctx(src, false, &HashSet::new())
    }

    fn classify_first_expr_in_loop(src: &str) -> Mapping {
        classify_first_expr_ctx(src, true, &HashSet::new())
    }

    fn classify_first_expr_ctx(
        src: &str,
        in_loop: bool,
        non_string_vars: &HashSet<String>,
    ) -> Mapping {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;
        let ctx = ClassifyCtx {
            in_loop,
            non_string_vars,
        };
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::ts()).parse();
        assert!(ret.diagnostics.is_empty(), "parse failed for {src:?}");
        let program = allocator.alloc(ret.program);
        for stmt in &program.body {
            if let oxc_ast::ast::Statement::ExpressionStatement(es) = stmt {
                return classify_expr(&es.expression, &ctx);
            }
        }
        panic!("no expression statement in {src:?}");
    }

    #[test]
    fn rejects_instanceof() {
        assert!(matches!(
            classify_first_expr("x instanceof Foo"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_delete() {
        assert!(matches!(
            classify_first_expr("delete o.x"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_reflection_globals() {
        assert!(matches!(classify_first_expr("Symbol"), Mapping::Reject(_)));
        assert!(matches!(classify_first_expr("Proxy"), Mapping::Reject(_)));
    }

    #[test]
    fn rejects_global_as_value() {
        assert!(matches!(classify_first_expr("Math"), Mapping::Reject(_)));
        assert!(matches!(classify_first_expr("Array"), Mapping::Reject(_)));
    }

    #[test]
    fn rejects_arguments_and_eval() {
        assert!(matches!(
            classify_first_expr("arguments"),
            Mapping::Reject(_)
        ));
        assert!(matches!(classify_first_expr("eval"), Mapping::Reject(_)));
    }

    #[test]
    fn rejects_bigint() {
        assert!(matches!(classify_first_expr("123n"), Mapping::Reject(_)));
    }

    #[test]
    fn rejects_await() {
        assert!(matches!(classify_first_expr("await p"), Mapping::Reject(_)));
    }

    #[test]
    fn rejects_constructor_reflection() {
        assert!(matches!(
            classify_first_expr("x.constructor"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_arity_reflection() {
        assert!(matches!(
            classify_first_expr("Math.floor.length"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_prototype_method_value() {
        assert!(matches!(
            classify_first_expr("Object.prototype.toString"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_accessor_properties() {
        assert!(matches!(
            classify_first_expr("({ get x() { return 1; } })"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_object_reflection_call() {
        assert!(matches!(
            classify_first_expr("Object.defineProperty({}, \"x\", { value: 1 })"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_reflect_namespace() {
        assert!(matches!(
            classify_first_expr("Reflect.has({}, \"x\")"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_string_raw() {
        assert!(matches!(
            classify_first_expr("String.raw({ raw: \"ab\" }, 1)"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_locale_aware_casing() {
        assert!(matches!(
            classify_first_expr("\"x\".toLocaleUpperCase(\"tr\")"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn rejects_has_own_property() {
        assert!(matches!(
            classify_first_expr("({}).hasOwnProperty(\"x\")"),
            Mapping::Reject(_)
        ));
    }

    #[test]
    fn degrades_regex_lastindex_read() {
        assert!(matches!(
            classify_first_expr("re.lastIndex"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_json_other() {
        assert!(matches!(
            classify_first_expr("JSON.rawJSON(\"1\")"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_function_iife() {
        assert!(matches!(
            classify_first_expr("(function () { return 1; })()"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_looped_regex_exec() {
        assert!(matches!(
            classify_first_expr_in_loop("re.exec(s)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn maps_once_regex_exec() {
        // A single `.exec` outside a loop stays on the regress path.
        assert!(classify_first_expr("re.exec(s)").is_mapped());
    }

    #[test]
    fn degrades_regex_test_non_string_literal() {
        assert!(matches!(
            classify_first_expr("re.test(123)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_regex_test_non_string_var() {
        let mut vars = HashSet::new();
        vars.insert("x".to_string());
        let m = classify_first_expr_ctx("re.test(x)", false, &vars);
        assert!(matches!(m, Mapping::DegradeEngine(_)));
    }

    #[test]
    fn maps_regex_test_string_var() {
        // An untracked binding may still be a string — do not route.
        assert!(classify_first_expr("re.test(x)").is_mapped());
    }

    #[test]
    fn degrades_array_includes_non_number() {
        assert!(matches!(
            classify_first_expr("[1, 2].includes(true)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn maps_array_includes_number() {
        assert!(classify_first_expr("[1, 2].includes(1)").is_mapped());
    }

    #[test]
    fn maps_plain_arithmetic() {
        assert!(classify_first_expr("1 + 2").is_mapped());
    }

    #[test]
    fn maps_static_call() {
        assert!(classify_first_expr("Math.floor(1.5)").is_mapped());
    }

    #[test]
    fn maps_json_parse() {
        assert!(classify_first_expr("JSON.parse(\"{}\")").is_mapped());
    }

    #[test]
    fn maps_prototype_value_read() {
        // `Array.prototype` itself is a mapped static-value read, not reflection.
        assert!(classify_first_expr("Array.prototype").is_mapped());
    }

    fn classify_fn(src: &str) -> Mapping {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::ts()).parse();
        assert!(ret.diagnostics.is_empty(), "parse failed for {src:?}");
        let program = allocator.alloc(ret.program);
        for stmt in &program.body {
            if let oxc_ast::ast::Statement::FunctionDeclaration(f) = stmt {
                return classify_function_signature(f);
            }
        }
        panic!("no function declaration in {src:?}");
    }

    fn classify_class_decl(src: &str) -> Mapping {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::ts()).parse();
        assert!(ret.diagnostics.is_empty(), "parse failed for {src:?}");
        let program = allocator.alloc(ret.program);
        for stmt in &program.body {
            if let oxc_ast::ast::Statement::ClassDeclaration(class) = stmt {
                return classify_class(class);
            }
        }
        panic!("no class declaration in {src:?}");
    }

    #[test]
    fn degrades_class_extends() {
        // `class B extends A` has no static lowering (composition only) → engine.
        assert!(matches!(
            classify_class_decl("class A extends B {}"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn maps_plain_class() {
        // A constructor + methods class stays on the static path (#130-132).
        assert!(classify_class_decl("class A { constructor() {} m() {} }").is_mapped());
    }

    #[test]
    fn degrades_unknown_param() {
        assert!(matches!(
            classify_fn("function f(x: unknown): void {}"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_any_param() {
        assert!(matches!(
            classify_fn("function f(x: any): void {}"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_record_of_unknown() {
        // `Record<string, unknown>` carries the untypable `unknown` in an
        // argument — recurse finds it.
        assert!(matches!(
            classify_fn("function f(x: Record<string, unknown>): void {}"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_indexed_access_return() {
        assert!(matches!(
            classify_fn("type O = { a: number }; function f(): O[\"a\"] { return 1; }"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn maps_concrete_signature() {
        assert!(matches!(
            classify_fn("function f(x: number, y: string): boolean { return true; }"),
            Mapping::Mapped
        ));
    }

    #[test]
    fn maps_union_of_concrete() {
        // A union of concrete members is expressible (it lowers to an enum).
        assert!(matches!(
            classify_fn("function f(x: string | number): void {}"),
            Mapping::Mapped
        ));
    }

    #[test]
    fn maps_nullable_union_param() {
        // `string | null` → `Option<String>`; the `null` is a nullable marker,
        // not unmappable — must not degrade.
        assert!(matches!(
            classify_fn("function f(x: string | null): void {}"),
            Mapping::Mapped
        ));
    }

    #[test]
    fn maps_return_type_typeof_query_param() {
        // `ReturnType<typeof g>` resolves in a signature position; the inner
        // `typeof` query must not trip the unmappable check.
        assert!(matches!(
            classify_fn("function f(x: ReturnType<typeof g>): void {}"),
            Mapping::Mapped
        ));
    }
}
