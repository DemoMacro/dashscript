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
    // test262 harness files under tests/conformance/data/harness/ (verbatim BSD
    // copies from tc39/test262) and the extracted corpus under
    // tests/conformance/data/test262/ (generated JSON — fixture bodies carry TS
    // source that is not oxlint's to reformat) are both excluded.
    "*": (files) => {
      const check = files.filter(
        (f) =>
          !f.includes("tests/conformance/data/harness/") &&
          !f.includes("tests/conformance/data/test262/"),
      );
      return check.length ? `vp check --fix ${check.join(" ")}` : "";
    },
    // Cargo gates run project-wide, not per-file: a GenerateTask returns the
    // command verbatim, so lint-staged skips appending staged paths (cargo fmt
    // and clippy reject extra file args). Fires only when a `*.rs` is staged.
    "*.rs": (): string[] => ["cargo fmt --check", "cargo clippy --all-targets -- -D warnings"],
  },
});
