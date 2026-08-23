# Agent orientation

These instructions apply to the entire repository. Keep this project a small,
durable, single Rust binary. Read `README.md` before changing behavior; it is the
user-facing contract and its deliberate exclusions are architectural boundaries,
not a backlog.

## Architecture

- `src/main.rs`: command dispatch, mutation orchestration, and terminal output.
- `src/cli.rs`: Clap command and argument definitions only.
- `src/inventory.rs`: persisted schema, normalization, lookup, and validation.
- `src/paths.rs`: platform paths, environment overrides, and OpenSSH `Include`
  inspection.
- `src/openssh.rs`: deterministic fragment generation plus all calls to the
  installed OpenSSH client.
- `src/store.rs`: locking, permissions, reads, and atomic inventory/config writes.
- `tests/cli.rs`: isolated end-to-end behavior using temporary directories and
  the `SSHLS_*` overrides.

Keep concerns in those modules. Do not introduce a library crate, daemon, async
runtime, database, SSH implementation, or general OpenSSH parser without an
explicit request to change the product boundary.

## Invariants

- `sshls.toml` is the source of truth. `sshls.conf` is deterministic derived
  output and must be repairable with `config sync`.
- Validate and normalize the complete inventory before writing anything.
- Preserve case-insensitive host-name uniqueness and deterministic host/tag
  ordering.
- Reject values that can inject OpenSSH directives or change meaning by working
  directory. Never interpolate unvalidated user text into generated config.
- Serialize mutations with the store lock. Preserve temporary-file, flush, and
  atomic-replacement behavior; a failed mutation must leave prior files intact.
- Preserve Unix modes and Windows ACL hardening for managed data.
- Ask the installed `ssh` binary when OpenSSH semantics matter. Spawn it directly,
  never through a shell, and retain process-level timeouts and noninteractive,
  known-host-safe connection flags.
- Persisted schema changes need an explicit compatibility/versioning decision.
  Never silently reinterpret an existing inventory version.
- Keep behavior and tests portable across Windows and Linux and compatible with
  Rust 1.85.

## Change workflow

Use the narrowest implementation that satisfies the request. Add or update tests
at the boundary being changed:

- CLI surface: `src/cli.rs`, dispatch in `src/main.rs`, README examples/help
  expectations, and an end-to-end test when observable.
- Inventory rules: focused unit tests in `src/inventory.rs` plus a failed-write
  assertion when persistence is involved.
- Generated config or OpenSSH behavior: unit tests in `src/openssh.rs`; keep exact
  output assertions deterministic.
- Paths/includes: unit tests in `src/paths.rs`, including quoting and Windows/Unix
  forms where relevant.
- Storage/durability: unit tests in `src/store.rs` and an integration regression
  test for externally visible guarantees.

Before handing off a change, run:

```console
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo build --release --locked
```

Do not use a developer's real SSH files in tests. Set `SSHLS_DIR`,
`SSHLS_USER_CONFIG`, and `SSHLS_SSH_BIN` as needed, and keep all fixtures in a
temporary directory.

## Dependencies and release contract

Prefer the standard library and existing dependencies. A new dependency needs a
clear maintenance or correctness benefit and must support the MSRV and both CI
platforms.

`Cargo.toml` is the release version source. For a release, update its `version`,
run `cargo check` to refresh `Cargo.lock`, commit both, and wait for CI on `main`.
Push an annotated tag with the exact version prefixed by `v`, for example:

```console
git tag -a v0.0.3 -m "sshls 0.0.3"
git push origin v0.0.3
```

Do not run `cargo publish` manually. `.github/workflows/release.yml` validates the
tag, publishes through crates.io trusted publishing, builds the Windows and static
Linux archives, generates checksums, and creates the matching GitHub release.
Published crates.io versions and pushed release tags are immutable; fix release
mistakes with a new patch version.
