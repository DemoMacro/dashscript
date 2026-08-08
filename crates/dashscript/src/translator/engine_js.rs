//! Lower a `.ts` program to plain ECMAScript via oxc's transformer so the
//! embedded QuickJS engine can evaluate it. Extracted from `translator/mod.rs`.

use oxc_codegen::Codegen;
use oxc_semantic::SemanticBuilder;

/// Lower a `.ts` program to plain ECMAScript via oxc's transformer
/// (preset-typescript): strip every type annotation and lower TS-only constructs
/// (`enum`, `namespace`, `import =`/`export =`) so the embedded QuickJS engine —
/// which parses JS, not TS — can evaluate it. The default `TransformOptions` runs
/// only the TypeScript pass (no ES-version downgrade; target is ESNext), keeping
/// modern syntax (for-of, arrow, class) as-is for QuickJS. Shared by the engine
/// lowering ([`Translator::translate_with_deps`]) and [`Translator::engine_source`]
/// (the conformance harness's direct-eval path), so both run the exact same bytes.
pub(super) fn engine_js_source<'a>(
    program: &mut oxc_ast::ast::Program<'a>,
    allocator: &'a oxc_allocator::Allocator,
    _scoping: oxc_semantic::Scoping,
) -> String {
    // oxc_transformer's TypeScript pass lowers `enum`/`namespace`/`import =`
    // by reading enum member values from the semantic graph, so it needs a
    // Scoping built with `with_enum_eval(true)`. The caller's scoping is built
    // for the name table (no enum eval), so rebuild here for the engine's JS.
    let scoping = SemanticBuilder::new()
        .with_enum_eval(true)
        .with_build_nodes(true)
        .build(&*program)
        .semantic
        .into_scoping();
    let transformer = oxc_transformer::Transformer::new(
        allocator,
        std::path::Path::new(""),
        &oxc_transformer::TransformOptions::default(),
    );
    transformer.build_with_scoping(scoping, program);
    Codegen::new().build(&*program).code
}
