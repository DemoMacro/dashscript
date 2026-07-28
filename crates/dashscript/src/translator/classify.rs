//! Translatability classification — the translator's single source of truth
//! for what it can lower to Rust. Each AST node maps to [`Mapping`]: lowered
//! to idiomatic Rust, rejected outright (no engine fallback), or degradable to
//! the embedded QuickJS engine. `check` and `program_uses_engine` will query
//! this rather than keeping a parallel rule tree — the drift that today lets a
//! new translator mapping not auto-relax a `check` rejection.
//!
//! Coverage today: the context-free rules (most `Reject` cases, plus the
//! `Function`-value / `JSON.<other>` / regex `.lastIndex` `DegradeEngine`
//! cases). Rules depending on traverse state (a regex `.exec` inside a loop, a
//! non-string regex/search argument bound elsewhere) return [`Mapping::Mapped`]
//! until a `ClassifyCtx` lands in a follow-up; the legacy `check` walk still
//! flags them, so behavior is unchanged. The Reject/DegradeEngine split is a
//! first pass toward per-function engine fallback — it mirrors today's
//! `needs-engine` lint messages and is refined as B6 lands.

use oxc_ast::ast::{
    BinaryOperator, CallExpression, Expression, ObjectPropertyKind, PropertyKind, UnaryOperator,
};

use super::globals::{is_global_receiver, is_static_only_global};

/// How a single AST node lowers — the translator's translatability verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapping {
    /// The translator lowers this node to idiomatic Rust.
    Mapped,
    /// No static lowering and no engine fallback — a hard `unsupported`
    /// (`instanceof`, `delete`, reflection globals, accessor properties,
    /// `BigInt` literal, `await`, prototype mutation, …).
    Reject,
    /// No static lowering, but the engine runs it verbatim (a `Function`
    /// value as a callee/argument, regex `.lastIndex`, `JSON.<method>` other
    /// than parse/stringify, …). The planned per-function fallback routes just
    /// the enclosing function to the engine.
    DegradeEngine,
}

/// Classify a single expression. Context-free: rules needing traverse state
/// (in-loop, non-string binding set) return [`Mapping::Mapped`] for now.
#[allow(dead_code)] // consumed once `check`/`program_uses_engine` switch to it
pub(super) fn classify_expr(expr: &Expression) -> Mapping {
    match expr {
        // TS type-layer wrappers and user parens carry no runtime meaning —
        // classify the inner expression.
        Expression::ParenthesizedExpression(p) => classify_expr(&p.expression),
        Expression::TSAsExpression(a) => classify_expr(&a.expression),
        Expression::TSTypeAssertion(t) => classify_expr(&t.expression),
        Expression::TSNonNullExpression(n) => classify_expr(&n.expression),

        // `x instanceof T` — a runtime type check with no static equivalent.
        Expression::BinaryExpression(b) if matches!(b.operator, BinaryOperator::Instanceof) => {
            Mapping::Reject
        }
        // `delete x` — no Rust analogue.
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Delete) => {
            Mapping::Reject
        }
        // Reflection/metaprogramming globals, `arguments`, `eval`, and a
        // global-object name read as a first-class value (the translator models
        // these only as a static-call/new receiver or type annotation).
        Expression::Identifier(id) => match id.name.as_str() {
            "Symbol" | "Proxy" | "WeakRef" | "FinalizationRegistry" | "arguments" | "eval" => {
                Mapping::Reject
            }
            name if is_static_only_global(name) => Mapping::Reject,
            _ => Mapping::Mapped,
        },
        // A reflection call, or a `Function`-value callee/argument. See
        // [`classify_call`] for the per-method breakdown.
        Expression::CallExpression(c) => classify_call(c),
        // `.constructor` — prototype reflection.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "constructor" => {
            Mapping::Reject
        }
        // `<re>.lastIndex` — the ES regex stateful cursor; regress is
        // stateless, so route to the engine.
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "lastIndex" => {
            Mapping::DegradeEngine
        }
        // `<Global>.<method>.length` — function arity reflection.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "length" && is_global_method_chain(&sm.object) =>
        {
            Mapping::Reject
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
            Mapping::Reject
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
                Mapping::Reject
            } else {
                Mapping::Mapped
            }
        }
        // `123n` — BigInt literals.
        Expression::BigIntLiteral(_) => Mapping::Reject,
        // `await expr` — DashScript has no async runtime.
        Expression::AwaitExpression(_) => Mapping::Reject,
        _ => Mapping::Mapped,
    }
}

/// Classify a call expression: reflection methods reject; a `Function` value
/// as callee/argument or `JSON.<other>` degrades to the engine.
fn classify_call(c: &CallExpression) -> Mapping {
    // A `function` expression as the callee (an IIFE) or as an argument (a
    // callback) has no static lowering — degrade to the engine.
    if is_function_expression(&c.callee)
        || c.arguments
            .iter()
            .any(|a| a.as_expression().is_some_and(is_function_expression))
    {
        return Mapping::DegradeEngine;
    }
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return Mapping::Mapped;
    };
    let prop = sm.property.name.as_str();
    // Instance prototype reflection methods.
    if matches!(
        prop,
        "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf"
    ) {
        return Mapping::Reject;
    }
    // `s.toLocaleUpperCase(locale)` / `toLocaleLowerCase(locale)` — locale-aware
    // casing with an explicit locale the locale-less mapping cannot honor.
    if matches!(prop, "toLocaleUpperCase" | "toLocaleLowerCase") && !c.arguments.is_empty() {
        return Mapping::Reject;
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
            return Mapping::Reject;
        }
        if obj.name.as_str() == "Reflect" {
            return Mapping::Reject;
        }
        if obj.name.as_str() == "String" && prop == "raw" {
            return Mapping::Reject;
        }
        // `JSON.<method>` other than parse/stringify (e.g. rawJSON/isRawJSON) —
        // no static mapping, so degrade to the engine, whose JSON matches ES.
        if obj.name.as_str() == "JSON" && !matches!(prop, "parse" | "stringify") {
            return Mapping::DegradeEngine;
        }
    }
    // TODO(B1b): regex `.exec` inside a loop, `.test`/`.exec` on a non-string,
    // and `.indexOf`/`.lastIndexOf`/`.includes` with a non-number needle need
    // traverse state (IN_LOOP / NON_STRING_VARS); Mapped until ClassifyCtx.
    Mapping::Mapped
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

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_first_expr(src: &str) -> Mapping {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, src, SourceType::ts()).parse();
        assert!(ret.diagnostics.is_empty(), "parse failed for {src:?}");
        let program = allocator.alloc(ret.program);
        for stmt in &program.body {
            if let oxc_ast::ast::Statement::ExpressionStatement(es) = stmt {
                return classify_expr(&es.expression);
            }
        }
        panic!("no expression statement in {src:?}");
    }

    #[test]
    fn rejects_instanceof() {
        assert_eq!(classify_first_expr("x instanceof Foo"), Mapping::Reject);
    }

    #[test]
    fn rejects_delete() {
        assert_eq!(classify_first_expr("delete o.x"), Mapping::Reject);
    }

    #[test]
    fn rejects_reflection_globals() {
        assert_eq!(classify_first_expr("Symbol"), Mapping::Reject);
        assert_eq!(classify_first_expr("Proxy"), Mapping::Reject);
    }

    #[test]
    fn rejects_global_as_value() {
        assert_eq!(classify_first_expr("Math"), Mapping::Reject);
        assert_eq!(classify_first_expr("Array"), Mapping::Reject);
    }

    #[test]
    fn rejects_bigint() {
        assert_eq!(classify_first_expr("123n"), Mapping::Reject);
    }

    #[test]
    fn rejects_constructor_reflection() {
        assert_eq!(classify_first_expr("x.constructor"), Mapping::Reject);
    }

    #[test]
    fn rejects_arity_reflection() {
        assert_eq!(classify_first_expr("Math.floor.length"), Mapping::Reject);
    }

    #[test]
    fn rejects_prototype_method_value() {
        assert_eq!(
            classify_first_expr("Object.prototype.toString"),
            Mapping::Reject
        );
    }

    #[test]
    fn rejects_accessor_properties() {
        assert_eq!(
            classify_first_expr("({ get x() { return 1; } })"),
            Mapping::Reject
        );
    }

    #[test]
    fn rejects_object_reflection_call() {
        assert_eq!(
            classify_first_expr("Object.defineProperty({}, \"x\", { value: 1 })"),
            Mapping::Reject
        );
    }

    #[test]
    fn rejects_reflect_namespace() {
        assert_eq!(
            classify_first_expr("Reflect.has({}, \"x\")"),
            Mapping::Reject
        );
    }

    #[test]
    fn rejects_string_raw() {
        assert_eq!(
            classify_first_expr("String.raw({ raw: \"ab\" }, 1)"),
            Mapping::Reject
        );
    }

    #[test]
    fn degrades_regex_lastindex() {
        assert_eq!(classify_first_expr("re.lastIndex"), Mapping::DegradeEngine);
    }

    #[test]
    fn degrades_json_other() {
        assert_eq!(
            classify_first_expr("JSON.rawJSON(\"1\")"),
            Mapping::DegradeEngine
        );
    }

    #[test]
    fn degrades_function_iife() {
        assert_eq!(
            classify_first_expr("(function () { return 1; })()"),
            Mapping::DegradeEngine
        );
    }

    #[test]
    fn maps_plain_arithmetic() {
        assert_eq!(classify_first_expr("1 + 2"), Mapping::Mapped);
    }

    #[test]
    fn maps_static_call() {
        assert_eq!(classify_first_expr("Math.floor(1.5)"), Mapping::Mapped);
    }

    #[test]
    fn maps_json_parse() {
        assert_eq!(classify_first_expr("JSON.parse(\"{}\")"), Mapping::Mapped);
    }

    #[test]
    fn maps_prototype_value_read() {
        // `Array.prototype` itself is a mapped static-value read, not reflection.
        assert_eq!(classify_first_expr("Array.prototype"), Mapping::Mapped);
    }
}
