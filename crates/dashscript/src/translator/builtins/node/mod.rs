//! Node standard-library mappings (`node:crypto`/`node:zlib`/`node:fs`/…).
//!
//! Empty today — Node stdlib support is on the roadmap, parallel to these ES
//! built-ins. When the first `node:` module lands, its bindings will live under
//! here (e.g. `node/crypto.rs` for `crypto.randomUUID()`), and a Node official
//! test suite will feed the conformance harness through the same `source`
//! field the tc39 test262 extractor already uses.
