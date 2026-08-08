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
fn degrades_instanceof() {
    assert!(matches!(
        classify_first_expr("x instanceof Foo"),
        Mapping::DegradeEngine(_)
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
fn degrades_static_only_global_as_value() {
    // A bare `Object`/`Math`/`Array` value has no static lowering, but the
    // engine ships them as globals — degrade (don't reject). A static-call
    // receiver (`Math.max(…)`) stays `Mapped` via a different arm.
    assert!(matches!(
        classify_first_expr("Math"),
        Mapping::DegradeEngine(_)
    ));
    assert!(matches!(
        classify_first_expr("Array"),
        Mapping::DegradeEngine(_)
    ));
    assert!(matches!(
        classify_first_expr("Object"),
        Mapping::DegradeEngine(_)
    ));
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
fn degrades_bigint() {
    // No static BigInt type, but QuickJS ships one — degrade.
    assert!(matches!(
        classify_first_expr("123n"),
        Mapping::DegradeEngine(_)
    ));
}

#[test]
fn maps_await() {
    // `await expr` lowers to `.await` inside an async fn (or the
    // `#[tokio::main] async fn main` a top-level await turns the entry
    // into); the bare operand stays Mapped.
    assert!(matches!(classify_first_expr("await p"), Mapping::Mapped));
}

#[test]
fn degrades_constructor_reflection() {
    assert!(matches!(
        classify_first_expr("x.constructor"),
        Mapping::DegradeEngine(_)
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
fn degrades_object_reflection_call() {
    // Property-descriptor / prototype-chain reflection has no static
    // lowering; the engine tracks ES property semantics natively.
    assert!(matches!(
        classify_first_expr("Object.defineProperty({}, \"x\", { value: 1 })"),
        Mapping::DegradeEngine(_)
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
fn degrades_has_own_property() {
    // Instance prototype reflection degrades — the engine tracks ES
    // property attributes natively.
    assert!(matches!(
        classify_first_expr("({}).hasOwnProperty(\"x\")"),
        Mapping::DegradeEngine(_)
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
        classify_first_expr("assert.throws(RangeError, () => Temporal.Duration.from('garbage'))"),
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
    let m = classify_first_expr_ctx("Temporal.PlainDate.compare(a, '2024-01-01')", false, &vars);
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
    let with_opts = classify_first_expr_ctx("x.toString({ calendarName: 'always' })", false, &vars);
    assert!(
        matches!(with_opts, Mapping::DegradeEngine(_)),
        "toString(options) must degrade: {with_opts:?}"
    );
    let bare = classify_first_expr_ctx("x.toString()", false, &vars);
    assert!(bare.is_mapped(), "bare toString stays static: {bare:?}");
}
