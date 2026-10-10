## Issue and milestone

- Closes #
- Milestone: MNN - Outcome

## Problem and resulting behavior

## Scope and exclusions

## Design and compatibility decisions

- ADRs:
- Assumptions:
- Known limitations:
- Dependency review (when dependencies change):
  - Required capability and owning layer:
  - Alternatives considered:
  - Enabled features and MSRV:
  - License, source, maintenance, and security status:
  - Dependency-tree change and exit path:

## Architecture

- Owning capability:
- Inbound adapter or caller:
- Consumed ports:
- Outbound adapters:
- Dependency edges added, removed, or excepted:
- Runnable end-to-end behavior:

## Verification

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
| `cargo test --workspace --all-features --locked` | | |
| `cargo doc --workspace --no-deps --locked` | | |

Checks not run, blocking conditions, and residual risk:

## Migration, rollout, and rollback

## Security and operations

## Review checklist

- [ ] One coherent capability; unrelated changes are excluded.
- [ ] Repository policy and architecture checks pass.
- [ ] Capability ownership and dependency changes are explicit and inward.
- [ ] Public and persisted contracts have a compatibility plan.
- [ ] Success and relevant invalid and failure paths are verified.
- [ ] Tenant, concurrency, idempotency, retry, and resource risks are covered.
- [ ] Documentation and ADRs are current.
- [ ] Migrations pass empty-database and supported-upgrade checks when relevant.
- [ ] No secrets, customer data, generated noise, or hidden skipped checks.
