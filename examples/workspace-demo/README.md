# workspace-demo

A DashScript monorepo. A root `manifest.json` with a `workspaces` glob lists the
members (`apps/*`), and `ds build` at the root compiles them under **one cargo
workspace** — a shared `target/` and `Cargo.lock`, so a dependency two members
use compiles once (cargo's hoisted-`node_modules`, the way pnpm hoists a shared
dep into the root `node_modules`).

```
workspace-demo/
  manifest.json          # { "workspaces": ["apps/*"] } — the workspace root
  apps/
    greeter/             # a member: its own manifest.json + main.ds
    counter/             # another member
```

## Build

```bash
$ ds build                       # each member → apps/<member>/dist/<member>
$ ds build --filter greeter      # build one member (name or directory)
```

Members cache together under the workspace's `.cache/dash/` (one cargo
workspace), cleared by a single `ds cache clean`. The plural `workspaces` field
mirrors npm/yarn/bun's `package.json`; pnpm instead uses a separate
`pnpm-workspace.yaml`.
