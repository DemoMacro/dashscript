import { defineConfig } from "vite-plus";

export default defineConfig({
  fmt: {
    sortImports: {
      type: "natural",
    },
    sortPackageJson: true,
    sortTailwindcss: {},
  },
  lint: {
    options: {
      typeAware: true,
      typeCheck: true,
    },
  },
  staged: {
    // DashScript source files: format + lint + type-check via vp check. The
    // test262 harness files under tests/conformance/data/harness/ are verbatim
    // BSD copies from tc39/test262 (third-party) — exclude them so their JSDoc
    // does not trip oxlint's type-aware rules and their formatting is preserved.
    "*": (files) => {
      const check = files.filter((f) => !f.includes("tests/conformance/data/harness/"));
      return check.length ? `vp check --fix ${check.join(" ")}` : "";
    },
    // Cargo gates run project-wide, not per-file: a GenerateTask returns the
    // command verbatim, so lint-staged skips appending staged paths (cargo fmt
    // and clippy reject extra file args). Fires only when a `*.rs` is staged.
    "*.rs": (): string[] => ["cargo fmt --check", "cargo clippy --all-targets -- -D warnings"],
  },
});
