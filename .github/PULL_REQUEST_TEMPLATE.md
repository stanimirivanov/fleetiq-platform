## Summary

Describe the problem and resulting behavior using the smallest useful diagram,
diff sketch or tree. Link the owning issue and relevant milestone.

Owning capability and dependency direction; tenant isolation, persisted/public contract compatibility, migration, rollout and rollback. For dependency changes, record capability, alternatives, features/MSRV, license/source, maintenance/security, dependency-tree change and exit path.

## Evidence

- **Before:** Reproduction, output or previous behavior.
- **After:** Verified result and exact commands.

Run the applicable acceptance gate and additional contract/image checks in CONTRIBUTING.md.

| Command or check | Result | Evidence or reason not run |
|:--|:--|:--|
| `cargo xtask repository` | | |
| `cargo xtask architecture` | | |
| `cargo xtask migrations --base origin/main` | | |
| `cargo xtask secrets --base origin/main` | | |
| `cargo xtask secrets --fixtures` | | |
| `cargo deny check` | | |
| `cargo machete --with-metadata --skip-target-dir` | | |
| `cargo fmt --all --check` | | |
| `cargo sqlx prepare --check --workspace -- --all-targets --all-features` | | |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | | |
| `cargo nextest run --workspace --all-features --locked --profile ci` | | |
| `cargo test --workspace --all-features --locked --doc` | | |
| `cargo doc --workspace --no-deps --locked` | | |

Checks not run, blocking conditions, and residual risk:

## Merge Danger

**Door:** one-way or two-way

**Blast Radius:** affected scope

Describe material compatibility, security and operational risks, limitations and
rollback. Evidence and checked boxes do not grant publication, merge or live
infrastructure authority.
