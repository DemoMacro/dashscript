import { defineConfig } from "vite-plus";

export default defineConfig({
  fmt: {
    sortImports: {
      type: "natural",
    },
    sortPackageJson: true,
    sortTailwindcss: {},
    // tests/conformance/data/ is upstream-generated test corpus (test262
    // harness, WPT fixtures, temporal polyfill) — never reformat it.
    ignorePatterns: ["**/tests/conformance/data/**"],
  },
  lint: {
    options: {
      typeAware: true,
      typeCheck: true,
    },
    // tests/conformance/data/ is upstream-generated test corpus (test262
    // harness, WPT fixtures, temporal polyfill) — not DashScript source, so
    // `vp check` must neither lint/type-check nor reformat it.
    ignorePatterns: ["**/tests/conformance/data/**"],
  },
  staged: {
    // DashScript source files: format + lint + type-check via vp check. The
    // entire tests/conformance/data/ tree is upstream-generated test corpus
    // (test262 harness, WPT fixtures, temporal polyfill) — a staged data file
    // never reaches oxlint.
    "*": (files) => {
      const check = files.filter((f) => !f.includes("tests/conformance/data/"));
      return check.length ? `vp check --fix ${check.join(" ")}` : "";
    },
    // Cargo gates run project-wide, not per-file: a GenerateTask returns the
    // command verbatim, so lint-staged skips appending staged paths (cargo fmt
    // and clippy reject extra file args). Fires only when a `*.rs` is staged.
    "*.rs": (): string[] => ["cargo fmt --check", "cargo clippy --all-targets -- -D warnings"],
  },
});
