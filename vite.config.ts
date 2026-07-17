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
    "*": "vp check --fix",
    // Cargo gates run project-wide, not per-file: a GenerateTask returns the
    // command verbatim, so lint-staged skips appending staged paths (cargo fmt
    // and clippy reject extra file args). Fires only when a `*.rs` is staged.
    "*.rs": (): string[] => ["cargo fmt --check", "cargo clippy --all-targets -- -D warnings"],
  },
});
