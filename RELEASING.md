# Releasing sshls

`Cargo.toml` is the source of truth for the version. A release tag must be the
same version prefixed with `v` (for example, version `0.0.1` uses tag `v0.0.1`).

The release workflow validates the package, builds a static Linux binary and a
Windows binary, publishes the crate, creates a GitHub release, and attaches the
two archives plus `SHA256SUMS`.

## One-time setup and the first release

crates.io requires a crate's first version to be published manually before a
GitHub trusted publisher can be configured.

1. Commit and push the release configuration to `main`, then wait for CI to
   pass.
2. In GitHub, create an environment named `release` under **Settings >
   Environments**. Optional deployment protection rules can be added there.
3. From a clean, up-to-date `main`, run:

   ```console
   cargo publish --dry-run --locked
   git tag -a v0.0.1 -m "sshls 0.0.1"
   cargo publish --locked
   ```

   Publishing to crates.io is permanent. Do not push the tag until the publish
   succeeds and the trusted publisher is configured.
4. In the new crate's crates.io settings, add a GitHub trusted publisher with:

   - owner: `xandwr`
   - repository: `sshls`
   - workflow: `release.yml`
   - environment: `release`

5. Start the automated release:

   ```console
   git push origin v0.0.1
   ```

The workflow recognizes that `0.0.1` is already on crates.io, verifies that its
package checksum matches the tag, and continues with the GitHub binary release.

## Future releases

1. Update `version` in `Cargo.toml` and run `cargo check` to refresh
   `Cargo.lock`.
2. Commit and push both files, then wait for CI to pass on `main`.
3. Create and push the matching annotated tag:

   ```console
   git tag -a v0.0.2 -m "sshls 0.0.2"
   git push origin v0.0.2
   ```

The tag is the only release trigger. If a workflow job fails, fix the underlying
problem and re-run the failed jobs in GitHub Actions. Never move or reuse a tag
for a version that has reached crates.io; publish a new patch version instead.
