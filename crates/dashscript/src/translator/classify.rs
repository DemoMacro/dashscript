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
use std::collections::HashMap;

use oxc_ast::ast::{
    Argument, AssignmentTarget, BinaryOperator, CallExpression, Class, Expression, Function,
    ObjectPropertyKind, PropertyKind, StaticMemberExpression, UnaryOperator,
};

use super::builtins::{
    temporal_callee_split, temporal_new_maps, temporal_static_maps, temporal_type_of_callee,
};
use super::globals::{
    is_engine_value_global, is_global_receiver, is_harness_helper, is_static_only_global,
    is_testharness_mapped, is_testharness_rejected, is_unmapped_new_global,
};

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
/// The kind of value a local in the walk is bound to — the type dimensions a
/// context-dependent classification reads. One table (rather than one field per
/// type-aware builtin) so adding a type-aware route — say a future typed-array
/// local — extends the enum, not the [`ClassifyCtx`] shape.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::translator) enum LocalKind {
    /// A plainly non-string literal initializer (number/boolean/object/array) —
    /// a `.test(x)`/`.exec(x)` on it needs the engine (ES coerces via ToString;
    /// regress takes `&str`).
    NonString,
    /// A string-literal initializer (`const s = "…"`). The emit's `is_string_arg`
    /// infers `String` for it, so `Temporal.<Type>.from(s)` stays on the static
    /// `from_utf8` path; an untracked local (an untyped callback parameter, an
    /// unknown) does not — it degrades so the polyfill carries the real coercion
    /// instead of the emit throwing a spurious `TypeError`.
    String,
    /// A `Temporal.<Type>.from(…)` / `new Temporal.<Type>(…)` initializer —
    /// carries the `<Type>` so `compare(a, b)` stays on the static
    /// `compare_iso(&a, &b)` path only when both operands are the same `<Type>`
    /// Temporal local, and `from(item)` degrades when `item` is one (ES clones
    /// it) so the polyfill carries the real ToTemporal coercion.
    Temporal(String),
}

#[derive(Debug, Clone, Copy)]
pub struct ClassifyCtx<'a> {
    /// True while classifying an expression inside a loop body or per-iteration
    /// condition — a `re.exec(…)` there needs the engine, because regress is
    /// stateless and would re-find the same match every iteration.
    pub in_loop: bool,
    /// Locals in the current walk whose initializer binds them to a known value
    /// kind, keyed by binding name. See [`LocalKind`].
    pub local_kinds: &'a HashMap<String, LocalKind>,
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
        // `arguments`/`eval`, and a global-object name read as a first-class
        // value. Reflection globals (`Symbol`/`Proxy`/`Reflect`/`WeakRef`/…)
        // and unmapped constructors (`Date`/`Promise`/`WeakMap`/…)
        // degrade to the engine — see [`ENGINE_VALUE_GLOBALS`]; only `arguments`
        // and `eval` (no engine substitute) stay rejected here.
        Expression::Identifier(id) => match id.name.as_str() {
            "arguments" => reject("the `arguments` object is unsupported"),
            "eval" => reject("`eval` is unsupported"),
            name if is_engine_value_global(name) => degrade_owned(format!(
                "`{name}` has no static mapping — the function runs under the engine"
            )),
            // A harness helper read as a value (`arr.map(compareArray)`,
            // `Reflect.apply(isConstructor, …)`) — same engine-only story as a
            // call to one.
            name if is_harness_helper(name) => degrade_owned(format!(
                "`{name}` is a test262 harness helper — runs under the engine"
            )),
            name if is_static_only_global(name) => reject_owned(format!(
                "`{name}` as a value is unsupported (use it only as a static-call/new receiver or \
                 type annotation)"
            )),
            _ => Mapping::Mapped,
        },
        // `this` outside a class method — the static emit is a `compile_error!`
        // (`this_expr`), so the enclosing function degrades to the engine
        // rather than failing `cargo build`. (A class method's `this` is `self`
        // and never reaches here — `collect_expr` does not walk class bodies, so
        // every `this` this arm sees is genuinely outside a method.) This is the
        // safety net for a `function` expression whose body now lowers to a
        // closure: `arr.map(function () { return this; })` would otherwise emit
        // `compile_error!` and break `ds build` (the conformance harness'
        // cargo-check-fail fallback is harness-only).
        Expression::ThisExpression(_) => {
            degrade("`this` outside a class method needs the engine (no static lowering)")
        }
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
        // `<Global>.prototype.<method>` — a prototype method read as a value
        // (e.g. `Array.prototype.map` passed as a callback or `.call`'d). No
        // static lowering exists, but the engine ships every builtin's
        // prototype method verbatim, so the enclosing function degrades.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() != "prototype"
                && matches!(
                    &sm.object,
                    Expression::StaticMemberExpression(outer)
                        if outer.property.name.as_str() == "prototype"
                            && is_global_object_receiver(&outer.object),
                ) =>
        {
            degrade("`<builtin>.prototype.<method>` reflection needs the engine")
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
        // A string literal carrying a real lone surrogate (`"\uD800"`) — oxc
        // decodes the surrogate to U+FFFD in `value` (Rust `&str` cannot hold
        // surrogates), so the static string diverges from ES and any regex/
        // char/length op on it is wrong. Degrade so QuickJS (which allows lone
        // surrogates) carries the real semantics. Only a *real* surrogate
        // degrades: oxc decodes both `\uD800` and a genuine `�` to the
        // same U+FFFD in `value`, so checking `value` over-degrades real-U+FFFD
        // fixtures (WPT `textdecoder-eof`'s expected `"�"`), which on the
        // no-engine WinterTC path turns a sound compile into an unsupported.
        // The raw source preserves the escape, so detect it there.
        Expression::StringLiteral(s) if raw_has_lone_surrogate(s.raw.as_deref()) => {
            degrade("a string literal with a lone surrogate needs the engine")
        }
        // `await expr` — lowers to Rust `.await` inside an `async fn` (or the
        // `#[tokio::main] async fn main` a top-level await turns the entry
        // into). The operand recurses, so `await <reflection>` still degrades.
        Expression::AwaitExpression(a) => classify_expr(&a.argument, ctx),
        // `new Temporal.<Type>(…)` — static ISO-field mapping (the four
        // date/time types) when the args are integer fields; a property-bag
        // `new Temporal.X({…})` or an unmapped type degrades to the engine,
        // whose polyfill carries the full ToTemporal<type> coercion.
        Expression::NewExpression(n) => match temporal_type_of_callee(&n.callee) {
            Some(ty) if temporal_new_maps(&ty) && !args_have_object(&n.arguments) => {
                Mapping::Mapped
            }
            Some(_) => {
                degrade("`new Temporal.*` property-bag/unmapped → engine (polyfill coercion)")
            }
            // `new Date()` — DashScript models Temporal (not `Date`), so the
            // `Date` constructor has no static mapping; the generic `Foo::new`
            // emit would produce a phantom `date` binding (E0433). Degrade so
            // the engine runs the real constructor (QuickJS ships a complete
            // `Date`). Scoped to `Date` alone: the other engine-value globals
            // (`Promise`/`DataView`/typed-array ctors) reach `new` only inside
            // reflection fixtures (`<X>.prototype.<m>.not-a-constructor`) that
            // already degrade via the `.constructor` reflection rule — adding a
            // `new`-site degrade there flips those fixtures off the engine path
            // (a per-function emit interaction), so they stay on the `None =>
            // Mapped` arm.
            None => match &n.callee {
                Expression::Identifier(id) if id.name.as_str() == "Date" => {
                    degrade("`new Date()` has no static mapping — runs under the engine")
                }
                // `new Array(n)` / `new ArrayBuffer(n)` / `new Object(x)` /
                // `new Number(x)` / … — ES builtin/wrapper constructors whose
                // `new` form has no static lowering (`new_expr` would emit
                // `Array::new(…)` → E0433 phantom type). The boxed wrapper /
                // sparse-array constructor semantics run natively under the
                // engine; none of these passes statically today, so degrading
                // cannot regress a static pass. See `UNMAPPED_NEW_GLOBALS`.
                Expression::Identifier(id) if is_unmapped_new_global(id.name.as_str()) => {
                    degrade_owned(format!(
                        "`new {name}()` has no static mapping — runs under the engine",
                        name = id.name.as_str()
                    ))
                }
                // `new <Identifier>(…)` — a user class or a mapped global ctor
                // (`RegExp`/`Worker`/`Uint8Array`/`TextEncoder`/`Error`,
                // lowered in `new_expr`). Static.
                Expression::Identifier(_) => Mapping::Mapped,
                // `new <member>(…)` / `new (expr)(…)` — `new_expr` lowers only
                // an Identifier callee; any other shape emits `todo!()` (a
                // runtime panic). ES built-in members (`Iterator.concat`,
                // `Promise.all`, …) are not constructors — `new` throws
                // TypeError — and a static lowering can neither build the value
                // nor decide [[Construct]]. The function runs under the engine,
                // where QuickJS applies real [[Construct]] semantics. No static
                // `new <member>` passes today (every one is `todo!()`), so this
                // cannot regress a static pass.
                _ => degrade("`new <non-identifier>(…)` has no static lowering → engine"),
            },
        },
        _ => Mapping::Mapped,
    }
}

/// True if any argument is an object literal — a Temporal property-bag coercion
/// (`from({year,month})`, `compare({…}, d)`, `new Temporal.X({…})`) the static
/// mapping cannot lower: `temporal_static` would emit a `TypeError`, and
/// `compare_iso` would hit a cargo type error. The engine's polyfill carries
/// the real `ToTemporalDate` coercion, so the enclosing call degrades.
fn args_have_object(args: &[Argument]) -> bool {
    args.iter()
        .any(|a| matches!(a, Argument::ObjectExpression(_)))
}

/// True when `Temporal.<ty>.<method>(args)` is statically compatible — the
/// static `temporal_rs` emit both compiles and produces correct ES behavior for
/// these operands. The pair itself is mapped (`temporal_static_maps`); this
/// decides argument compatibility, the half a context-free classify cannot.
///
/// - `from(item)` — the emit (`temporal_from`) parses only a string operand;
///   any other operand needs the polyfill's full ToTemporal coercion (a
///   Temporal local → ES clones it; a number → ES TypeError that may be
///   asserted; an object → property-bag). Mirrors the translator's `is_string_arg`.
/// - `compare(a, b)` — the emit (`temporal_compare`) lowers `compare_iso(&a,
///   &b)` / `.cmp(&a, &b)`, so both operands must be `<ty>` Temporal values;
///   a string literal, a non-Temporal local, or a Temporal local of a different
///   type would fail cargo check or mis-compare.
/// - `fromEpochMilliseconds(n)` takes a `number` (`f64`) — always static.
fn temporal_args_static_compatible(
    ty: &str,
    method: &str,
    args: &[Argument],
    ctx: &ClassifyCtx,
) -> bool {
    match method {
        "from" => !from_arg_needs_engine(args.first(), ctx),
        "compare" => {
            let a = compare_operand_type(args.first(), ctx);
            let b = compare_operand_type(args.get(1), ctx);
            a.as_deref() == Some(ty) && b.as_deref() == Some(ty)
        }
        _ => true,
    }
}

/// True when a `Temporal.<Type>.from(item)` operand is plainly not a string —
/// a non-string literal, `undefined`, or a local bound (in this walk) to one or
/// to a Temporal value. Only a string operand (a string literal, or an
/// untracked local the emit infers as `String`) stays on the static `from_utf8`
/// path; anything else degrades so the polyfill carries the real coercion.
fn from_arg_needs_engine(arg: Option<&Argument>, ctx: &ClassifyCtx) -> bool {
    let Some(expr) = arg.and_then(|a| a.as_expression()) else {
        return false;
    };
    match expr {
        Expression::StringLiteral(_) => false,
        Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::ObjectExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::NullLiteral(_) => true,
        // `void <expr>` evaluates to `undefined` → ToTemporal rejects it.
        Expression::UnaryExpression(u) if matches!(u.operator, UnaryOperator::Void) => true,
        Expression::Identifier(id) => {
            if id.name.as_str() == "undefined" {
                return true;
            }
            // Only a local bound to a string literal stays on the static
            // `from_utf8` path (the emit's `is_string_arg` infers `String` for
            // it); a Temporal/NonString local, or an untracked local (an
            // untyped callback parameter, an unknown) needs the polyfill. The
            // polyfill (@js-temporal/polyfill) is the ES reference and is MORE
            // conformant than temporal-rs on edge-case ISO strings (minus-sign,
            // calendar annotations, UTC designators), so `for (const s of arr)
            // from(s)` that degrades runs more fixtures supported than the
            // static temporal-rs path — the untracked loop variable routes to
            // the engine on purpose.
            !matches!(
                ctx.local_kinds.get(id.name.as_str()),
                Some(LocalKind::String)
            )
        }
        // Any other expression (a template literal, a call, a member access, …)
        // — the emit's `is_string_arg` only parses a `StringLiteral`/`String`-
        // inferred identifier, so it would throw a spurious `TypeError` for
        // these. Degrade so the polyfill carries the real ToTemporal coercion.
        _ => true,
    }
}

/// The Temporal `<Type>` of a `compare` operand, when it is a local bound (in
/// this walk) to a Temporal value of that type. Returns `None` for a string
/// literal, a non-Temporal local, a Temporal local of any type (the caller
/// checks it matches `<ty>`), or any other expression — `compare_iso`/`.cmp`
/// need `&<ty>` operands, so only a same-type Temporal local stays static.
fn compare_operand_type(arg: Option<&Argument>, ctx: &ClassifyCtx) -> Option<String> {
    let expr = arg.and_then(|a| a.as_expression())?;
    let Expression::Identifier(id) = expr else {
        return None;
    };
    match ctx.local_kinds.get(id.name.as_str())? {
        LocalKind::Temporal(ty) => Some(ty.clone()),
        LocalKind::NonString | LocalKind::String => None,
    }
}

/// The Temporal `<Type>` of a `<recv>.<method>` receiver, when `recv` is a
/// local bound to a Temporal value of that type (in this walk). `None` for a
/// non-Temporal local or any other receiver shape.
fn temporal_receiver_ty(sm: &StaticMemberExpression, ctx: &ClassifyCtx) -> Option<String> {
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    match ctx.local_kinds.get(id.name.as_str())? {
        LocalKind::Temporal(ty) => Some(ty.clone()),
        LocalKind::NonString | LocalKind::String => None,
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
    // A `function` expression (IIFE callee or callback argument) lowers
    // statically to a closure (`function_expr_to_closure`), the same shape a
    // block-body arrow takes — so it no longer degrades. A body using
    // `this`/`arguments`/`super` keeps the closure shape but its `this` emits a
    // `compile_error!`; cargo check then fails and the enclosing function
    // degrades to the engine (the static path stays for the common callback).
    // `Temporal.<Type>.<method>(…)` — route to the static `temporal_rs` mapping
    // when the pair is mapped AND the args are type-compatible with the static
    // emit (a `from(item)` only for a string operand; a `compare(a, b)` only
    // when both `a` and `b` are same-`<Type>` Temporal locals). An unmapped pair
    // or a type-mismatched operand degrades to the engine, whose polyfill
    // carries the full ToTemporal coercion and every unmapped method — the
    // static emit would otherwise mis-lower (`from(temporal)` panics TypeError
    // where ES clones) or fail cargo check (`compare_iso` needs `&<ty>`).
    // Static-first: the zero-cost path wins where it genuinely can.
    if let Some((ty, method)) = temporal_callee_split(&c.callee) {
        if temporal_static_maps(ty, method)
            && temporal_args_static_compatible(ty, method, &c.arguments, ctx)
        {
            return Mapping::Mapped;
        }
        return degrade("Temporal.* unmapped/type-mismatched coercion → engine (polyfill)");
    }
    // Bare `assert(mustBeTrue[, message])` — test262's `assert` passes iff
    // `mustBeTrue === true` (strict, per assert.js), i.e. exactly
    // `assert.sameValue(mustBeTrue, true)`, so it lowers statically. Keeping
    // these fixtures off the engine matters where the engine cannot parse the
    // source's ES2025 regex (`(?s:…)` modifiers, duplicate named groups):
    // regress parses them, QuickJS-NG does not — the engine path would
    // SyntaxError where the static path runs the assert.
    if let Expression::Identifier(id) = &c.callee {
        if id.name.as_str() == "assert" {
            return Mapping::Mapped;
        }
        // WPT testharness globals — `test()`/`assert_equals`/… (the
        // web-platform analogue of test262's `assert`). The mapped set lowers
        // statically to `__ds::wpt_*` (WinterTC is pure-Rust, no degradation);
        // the rejected set (async/composite) has no static lowering and, per
        // WinterTC's static-only contract, no engine fallback — honestly
        // `unsupported` rather than degraded.
        if is_testharness_rejected(id.name.as_str()) {
            return reject_owned(format!(
                "`{name}` is a WPT testharness function with no static lowering (async/composite) \
                 — WinterTC is static-only, no degradation",
                name = id.name.as_str()
            ));
        }
        if is_testharness_mapped(id.name.as_str()) {
            return Mapping::Mapped;
        }
        // A test262 harness helper (`isConstructor`, `compareArray`,
        // `verifyProperty`, `testWithTypedArrayConstructors`, …) is defined only
        // in a `$INCLUDE` the engine injects; the static emit would snake-case
        // the name into a phantom binding (E0425). Degrade so the engine runs it
        // with the harness injected.
        if is_harness_helper(id.name.as_str()) {
            return degrade_owned(format!(
                "`{name}` is a test262 harness helper — runs under the engine",
                name = id.name.as_str()
            ));
        }
    }
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return Mapping::Mapped;
    };
    let prop = sm.property.name.as_str();
    // `<temporal>.toString({options})` / `<temporal>.toJSON({options})` — the
    // static `Display` emit ignores the options bag (`calendarName` /
    // `fractionalSecondDigits` / `roundingMode` / …), so a call WITH arguments
    // degrades to the engine, whose polyfill honours them. A bare
    // `<temporal>.toString()` stays on the static `Display` path.
    if matches!(prop, "toString" | "toJSON") && !c.arguments.is_empty() {
        if let Some(ty) = temporal_receiver_ty(sm, ctx) {
            return degrade_owned(format!(
                "`{ty}.{{toString|toJSON}}(options)` — the static `Display` emit ignores the \
                 options bag — runs under the engine"
            ));
        }
    }
    // `Promise.resolve(x)` / `Promise.all([...])` — static combinators with a
    // native Rust lowering (T3 stage 2a: `ds_promise_resolve`/`ds_promise_all`).
    // `Promise` is otherwise an engine-value global (bare `Promise`, `new
    // Promise`, `Promise.race`/`.then`, …) — only these two have a static emit,
    // so they are pulled out before the engine degrade.
    if let Expression::Identifier(id) = &sm.object {
        if id.name.as_str() == "Promise" && matches!(prop, "resolve" | "all") {
            return Mapping::Mapped;
        }
    }
    // `<engine-value-global>.<method>(…)` — these globals (`Date`, `Promise`,
    // `Atomics`, the BigInt typed-array constructors, the test262
    // `TemporalHelpers`/`$262` harness objects, …) carry no static member-call
    // mapping; the generic member-call emit snake-cases the receiver name and
    // produces a phantom binding (E0425). Degrade so the engine runs the real
    // method (QuickJS ships the ES ones; the harness ones arrive via the
    // injected `includes`). The integer/float typed-array ctors (`Int32Array`,
    // …) are absent — their `new`/type map, so a `.from`/`.of` member call
    // falls through to the cargo-check-fail fallback. `Temporal` is routed
    // earlier by `temporal_callee_split`, so a `Temporal.<Type>.<method>` call
    // never reaches this arm.
    if let Expression::Identifier(id) = &sm.object {
        if is_engine_value_global(id.name.as_str()) {
            // Name the global in the message (e.g. `` `Reflect.<method>` ``) so
            // the diagnostic and test assertions can match on it — the earlier
            // generic `<engine-value-global>` placeholder hid which global fired.
            let name = id.name.as_str();
            return degrade_owned(format!(
                "`{name}.<method>` has no static mapping — runs under the engine"
            ));
        }
    }
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
        // `Object.freeze`/`seal`/`preventExtensions` mutate, and `isFrozen`/
        // `isSealed`/`isExtensible` query, an object's [[Extensible]]/property
        // attribute state. A DashScript `Record`/struct carries no runtime
        // freeze flag, so the static emit is a no-op (`freeze` → `clone`) or
        // hardcoded (`isExtensible` → `true`) — a fixture that freezes then
        // asserts `isExtensible` is `false` mis-reports. The engine tracks ES
        // extensibility natively, so degrade the enclosing function.
        let is_object_freeze = matches!(
            prop,
            "freeze" | "seal" | "preventExtensions" | "isFrozen" | "isSealed" | "isExtensible"
        );
        if obj.name.as_str() == "Object" && is_object_freeze {
            return degrade_owned(format!(
                "`Object.{prop}` (extensibility state) needs the engine (no static freeze tracking)"
            ));
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
/// operands lower to a Rust SameValue check; `throws(constructor, fn)` lowers
/// to a catch_unwind + error-class check; everything else (`compareArray`,
/// reflection helpers, or a composite operand whose ES SameValue is reference
/// identity) degrades to the engine, where `assert.js`/`propertyHelper.js` run
/// natively.
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
        // `assert.throws(Ctor, fn[, msg])` — the static `__ds::assert_throws`
        // catch_unwinds `fn` and checks the panic's `DsError` class matches
        // `Ctor`. test262 invokes `fn` with zero arguments, so only a zero-param
        // arrow callback lowers (to a `FnOnce() -> R` closure); a parametrized
        // callback (its param would be `undefined`) needs the engine. A `function`
        // callback never reaches here — `classify_call` degrades any call with a
        // function-expression argument first. A non-Identifier constructor (e.g.
        // a member expression) likewise degrades.
        "throws" => {
            let ctor_is_ident = matches!(
                args.first().and_then(|a| a.as_expression()),
                Some(Expression::Identifier(_))
            );
            let fn_is_zero_param_arrow = args.get(1).is_some_and(|a| {
                matches!(
                    a.as_expression(),
                    Some(Expression::ArrowFunctionExpression(arrow)) if arrow.params.items.is_empty()
                )
            });
            if ctor_is_ident && fn_is_zero_param_arrow {
                Mapping::Mapped
            } else {
                degrade(
                    "`assert.throws` non-Identifier constructor or non-zero-param callback needs the engine",
                )
            }
        }
        // `compareArray`/`verifyProperty`/… — the engine runs the test262
        // harness (`assert.js`/`propertyHelper.js`/`compareArray.js`) natively.
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
            matches!(
                ctx.local_kinds.get(id.name.as_str()),
                Some(LocalKind::NonString)
            )
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

/// True when a JS string literal's *raw source* carries a lone-surrogate
/// `\u`-escape (`\uD800`–`\uDFFF`, braced or 4-hex). oxc decodes a lone
/// surrogate to U+FFFD in the literal's `value` (Rust `&str` cannot hold
/// surrogates), which is indistinguishable there from a genuine `�` — so
/// `value.contains(U+FFFD)` over-degrades real-U+FFFD fixtures. The raw source
/// preserves the original escape, so a lone surrogate is detectable there. `None`
/// (no raw) conservatively degrades — oxc always carries raw for a parsed
/// literal, so this only fires for synthesized nodes.
fn raw_has_lone_surrogate(raw: Option<&str>) -> bool {
    let Some(raw) = raw else { return true };
    let b = raw.as_bytes();
    // Collect (start, end, code_point) for every `\u…` escape, in source order.
    let mut esc: Vec<(usize, usize, u32)> = Vec::new();
    let mut i = 0;
    while i + 2 < b.len() {
        if b[i] == b'\\' && b[i + 1] == b'u' {
            // (hex_slice_start, hex_slice_end, escape_end_after) — `after` is
            // the index past the whole escape, so adjacency (a high surrogate
            // immediately followed by a low one, nothing between) compares
            // `esc[idx].1 == esc[idx+1].0`.
            let (hs, he, after) = if b[i + 2] == b'{' {
                match raw[i + 3..].find('}') {
                    Some(c) => (i + 3, i + 3 + c, i + 3 + c + 1),
                    None => break,
                }
            } else if i + 5 < b.len() && b[i + 2..i + 6].iter().all(|c| c.is_ascii_hexdigit()) {
                (i + 2, i + 6, i + 6)
            } else {
                i += 1;
                continue;
            };
            if let Ok(cp) = u32::from_str_radix(&raw[hs..he], 16) {
                esc.push((i, after, cp));
            }
            i = after;
        } else {
            i += 1;
        }
    }
    // A high surrogate (0xD800–0xDBFF) immediately followed — adjacent in the
    // source, nothing between the two escapes — by a low surrogate
    // (0xDC00–0xDFFF) is a valid UTF-16 pair (e.g. `😀` = 😀) that decodes to
    // a scalar value Rust `&str` can hold, so it is NOT lone. Any other
    // surrogate is a lone surrogate (Rust `&str` cannot represent it); oxc
    // decodes a lone surrogate in `value` to U+FFFD, which is why the raw
    // source — not the decoded value — is the only place the two can be told
    // apart (see the `StringLiteral` arm above).
    let mut idx = 0;
    while idx < esc.len() {
        let &(_s, end, cp) = &esc[idx];
        if (0xD800..=0xDBFF).contains(&cp) {
            if idx + 1 < esc.len() {
                let &(ns, _ne, ncp) = &esc[idx + 1];
                if ns == end && (0xDC00..=0xDFFF).contains(&ncp) {
                    idx += 2; // valid pair — skip both
                    continue;
                }
            }
            return true; // lone high surrogate
        } else if (0xDC00..=0xDFFF).contains(&cp) {
            return true; // lone low surrogate (a paired low is consumed above)
        }
        idx += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_first_expr(src: &str) -> Mapping {
        classify_first_expr_ctx(src, false, &HashMap::new())
    }

    fn classify_first_expr_in_loop(src: &str) -> Mapping {
        classify_first_expr_ctx(src, true, &HashMap::new())
    }

    #[test]
    fn raw_surrogate_pair_vs_lone() {
        // Valid UTF-16 pairs (emoji etc.) decode to a scalar Rust `&str` can
        // hold — NOT lone, regardless of escape form.
        assert!(!raw_has_lone_surrogate(Some(r#"😀"#))); // 😀
        assert!(!raw_has_lone_surrogate(Some(r#"\u{D83D}\u{DE00}"#)));
        assert!(!raw_has_lone_surrogate(Some(r#"😀!"#))); // pair + ascii
                                                          // Lone surrogates — Rust `&str` cannot represent them.
        assert!(raw_has_lone_surrogate(Some(r#"\uD800"#))); // lone high
        assert!(raw_has_lone_surrogate(Some(r#"\uDE00"#))); // lone low
        assert!(raw_has_lone_surrogate(Some(r#"\u{D800}"#))); // braced lone high
        assert!(raw_has_lone_surrogate(Some(r#"\uD83Dx\uDE00"#))); // split (not adjacent)
        assert!(raw_has_lone_surrogate(Some(r#"\uD800\uD900"#))); // two highs
                                                                  // A genuine U+FFFD escape is NOT a surrogate.
        assert!(!raw_has_lone_surrogate(Some(r#"�"#)));
        assert!(!raw_has_lone_surrogate(Some(r#"\u{FFFD}"#)));
        // No raw ⇒ cannot prove either way, treat as needing the engine.
        assert!(raw_has_lone_surrogate(None));
        // Plain ascii, nothing to flag.
        assert!(!raw_has_lone_surrogate(Some(r#"hello"#)));
    }

    fn classify_first_expr_ctx(
        src: &str,
        in_loop: bool,
        local_kinds: &HashMap<String, LocalKind>,
    ) -> Mapping {
        use oxc_allocator::Allocator;
        use oxc_parser::Parser;
        use oxc_span::SourceType;
        let ctx = ClassifyCtx {
            in_loop,
            local_kinds,
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
    fn degrades_reflection_globals() {
        // Symbol/Proxy/Reflect/WeakRef/… degrade to the engine (QuickJS ships
        // them) rather than rejecting — see ENGINE_VALUE_GLOBALS.
        assert!(matches!(
            classify_first_expr("Symbol"),
            Mapping::DegradeEngine(_)
        ));
        assert!(matches!(
            classify_first_expr("Proxy"),
            Mapping::DegradeEngine(_)
        ));
        assert!(matches!(
            classify_first_expr("Reflect"),
            Mapping::DegradeEngine(_)
        ));
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
    fn maps_await() {
        // `await expr` lowers to `.await` inside an async fn (or the
        // `#[tokio::main] async fn main` a top-level await turns the entry
        // into); the bare operand stays Mapped.
        assert!(matches!(classify_first_expr("await p"), Mapping::Mapped));
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
    fn degrades_prototype_method_value() {
        // `<builtin>.prototype.<method>` reads a builtin method as a value — no
        // static lowering, but the engine ships it, so the function degrades.
        assert!(matches!(
            classify_first_expr("Object.prototype.toString"),
            Mapping::DegradeEngine(_)
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
    fn degrades_reflect_member_call() {
        // `<engine-value-global>.<method>` (e.g. `Reflect.has`) has no static
        // member-call mapping — degrade so the engine runs the real method
        // (QuickJS ships `Reflect`). The bare-value form is covered by
        // `degrades_reflection_globals` above.
        assert!(matches!(
            classify_first_expr("Reflect.has({}, \"x\")"),
            Mapping::DegradeEngine(_)
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
    fn maps_function_iife() {
        // A `function` expression (IIFE callee or callback) lowers to a closure
        // (`function_expr_to_closure`), the same shape a block-body arrow takes,
        // so it stays mapped rather than degrading to the engine.
        assert!(classify_first_expr("(function () { return 1; })()").is_mapped());
    }

    #[test]
    fn degrades_this_outside_method() {
        // `this` outside a class method has no static lowering — the static emit
        // is `compile_error!`, so it degrades to the engine. This is the safety
        // net for a `function`-expression callback whose body now lowers to a
        // closure: `function () { return this; }` would otherwise break `ds build`.
        // (`collect_expr` recurses the IIFE body in the `program_uses_engine`
        // walk, so the `this` inside is caught there; here the bare `this` tests
        // the arm directly.)
        assert!(matches!(
            classify_first_expr("this"),
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
        let mut vars = HashMap::new();
        vars.insert("x".to_string(), LocalKind::NonString);
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

    #[test]
    fn maps_assert_throws_zero_param_arrow() {
        // test262 invokes the callback with zero args, so a zero-param arrow
        // lowers to a `FnOnce() -> R` closure → static `__ds::assert_throws`.
        assert!(matches!(
            classify_first_expr(
                "assert.throws(RangeError, () => Temporal.Duration.from('garbage'))"
            ),
            Mapping::Mapped
        ));
    }

    #[test]
    fn degrades_assert_throws_param_callback() {
        // A parametrized callback (its param would be `undefined` when test262
        // calls it) cannot lower to `FnOnce() -> R` → engine.
        assert!(matches!(
            classify_first_expr("assert.throws(RangeError, (e) => 1)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_assert_throws_non_identifier_ctor() {
        // A non-Identifier constructor (a member expression) carries no static
        // class name → engine.
        assert!(matches!(
            classify_first_expr("assert.throws(Error.SubType, () => 1)"),
            Mapping::DegradeEngine(_)
        ));
    }

    // --- Temporal: type-aware static-vs-engine routing ---------------------
    //
    // `Temporal.<Type>.<method>` routes to the static `temporal_rs` path only
    // when the operands are type-compatible with the emit: `from(item)` needs a
    // string; `compare(a, b)` needs two same-`<Type>` Temporal locals. Anything
    // else degrades so the polyfill carries the real ToTemporal coercion.

    #[test]
    fn maps_temporal_from_string_literal() {
        // A string operand parses via `from_utf8` — the zero-cost static path.
        assert!(classify_first_expr("Temporal.PlainDate.from('2024-03-15')").is_mapped());
    }

    #[test]
    fn degrades_temporal_from_untracked_local() {
        // An untracked local's type is unknown to the walk, so degrade and let
        // the polyfill carry the real ToTemporal coercion. The polyfill
        // (@js-temporal/polyfill) is the ES reference and is MORE conformant
        // than temporal-rs on edge-case ISO strings (minus-sign, calendar
        // annotations, UTC designators): a `for (const s of arr) from(s)` that
        // degrades runs more fixtures supported than the static temporal-rs
        // path — quantified at -50 fixtures when the loop variable was forced
        // static. A local bound to a string literal stays static.
        assert!(matches!(
            classify_first_expr("Temporal.PlainDate.from(s)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_temporal_from_number() {
        // A number operand → ES TypeError; degrade so the polyfill coerces.
        assert!(matches!(
            classify_first_expr("Temporal.PlainDate.from(123)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_temporal_from_property_bag() {
        // A property-bag object → ToTemporal coercion; only the polyfill has it.
        assert!(matches!(
            classify_first_expr("Temporal.PlainDate.from({ year: 2024, month: 3, day: 15 })"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_temporal_from_temporal_local() {
        // `from(temporal)` clones in ES; the static emit would panic TypeError.
        let mut vars = HashMap::new();
        vars.insert(
            "x".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        let m = classify_first_expr_ctx("Temporal.PlainDate.from(x)", false, &vars);
        assert!(matches!(m, Mapping::DegradeEngine(_)));
    }

    #[test]
    fn degrades_temporal_from_non_string_local() {
        // A local bound to a non-string literal → not a string → engine.
        let mut non_string = HashMap::new();
        non_string.insert("n".to_string(), LocalKind::NonString);
        let m = classify_first_expr_ctx("Temporal.PlainDate.from(n)", false, &non_string);
        assert!(matches!(m, Mapping::DegradeEngine(_)));
    }

    #[test]
    fn maps_temporal_compare_same_type_locals() {
        // Two same-`<Type>` Temporal locals → static `compare_iso(&a, &b)`.
        let mut vars = HashMap::new();
        vars.insert(
            "a".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        vars.insert(
            "b".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        let m = classify_first_expr_ctx("Temporal.PlainDate.compare(a, b)", false, &vars);
        assert!(m.is_mapped());
    }

    #[test]
    fn degrades_temporal_compare_untracked_locals() {
        // Untracked locals would fail cargo check — `compare_iso` needs
        // `&PlainDate`, not whatever `a`/`b` translate to.
        assert!(matches!(
            classify_first_expr("Temporal.PlainDate.compare(a, b)"),
            Mapping::DegradeEngine(_)
        ));
    }

    #[test]
    fn degrades_temporal_compare_string_operand() {
        // A string literal operand (not a Temporal local) → degrade.
        let mut vars = HashMap::new();
        vars.insert(
            "a".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        let m =
            classify_first_expr_ctx("Temporal.PlainDate.compare(a, '2024-01-01')", false, &vars);
        assert!(matches!(m, Mapping::DegradeEngine(_)));
    }

    #[test]
    fn degrades_temporal_compare_mismatched_types() {
        // Two Temporal locals of different types → cargo check fail → degrade.
        let mut vars = HashMap::new();
        vars.insert(
            "a".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        vars.insert(
            "b".to_string(),
            LocalKind::Temporal("PlainDateTime".to_string()),
        );
        let m = classify_first_expr_ctx("Temporal.PlainDate.compare(a, b)", false, &vars);
        assert!(matches!(m, Mapping::DegradeEngine(_)));
    }

    #[test]
    fn degrades_temporal_to_string_with_options() {
        // `<temporal>.toString({options})` — the static `Display` emit ignores
        // the options bag (`calendarName` / `fractionalSecondDigits` /
        // `roundingMode` / …), so a call WITH arguments degrades to the engine,
        // whose polyfill honours them. A bare `<temporal>.toString()` stays on
        // the static `Display` path.
        let mut vars = HashMap::new();
        vars.insert(
            "x".to_string(),
            LocalKind::Temporal("PlainDate".to_string()),
        );
        let with_opts =
            classify_first_expr_ctx("x.toString({ calendarName: 'always' })", false, &vars);
        assert!(
            matches!(with_opts, Mapping::DegradeEngine(_)),
            "toString(options) must degrade: {with_opts:?}"
        );
        let bare = classify_first_expr_ctx("x.toString()", false, &vars);
        assert!(bare.is_mapped(), "bare toString stays static: {bare:?}");
    }
}
