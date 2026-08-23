# sshls

`sshls` is a small inventory for SSH destinations. It owns a deliberately narrow TOML file, generates an OpenSSH fragment, and asks the installed `ssh` client whenever OpenSSH semantics matter.

It does not parse, replace, or rewrite your existing SSH configuration.

## Install

Build with Rust 1.85 or newer:

```console
cargo install --path .
```

Generate the initial fragment:

```console
sshls config sync
```

Then place the line printed by that command at the beginning of your user SSH config. At the default location it is:

```sshconfig
Include ~/.ssh/sshls.conf
```

Putting the include first makes sshls-managed values authoritative for managed aliases. OpenSSH still supplies any other settings from later matching blocks.

Run `sshls config check` to verify the complete setup.

## Use

```console
# Add and inspect destinations
sshls add prod-web 203.0.113.10 --user deploy --port 22 \
  --identity ~/.ssh/id_ed25519 --tag prod --tag web
sshls
sshls show prod-web
sshls find prod

# Ask OpenSSH what it will really use, or test a connection
sshls resolve prod-web
sshls test prod-web
sshls test prod-web --timeout 20

# Change the inventory
sshls edit prod-web --user admin
sshls edit prod-web --clear-port --clear-identity
sshls clone prod-web staging-web
sshls rename staging-web stage-web
sshls tag add prod-web critical
sshls tag rm prod-web web
sshls rm stage-web

# Diagnose or repair generated output
sshls config check
sshls config sync
```

Use `sshls <command> --help` for complete command syntax.

## Files and guarantees

At the default location, sshls owns only:

- `~/.ssh/sshls.toml` — the source of truth
- `~/.ssh/sshls.conf` — deterministic generated OpenSSH configuration
- `~/.ssh/.sshls.lock` — concurrency lock

Inventory writes are validated, serialized across processes, written through a temporary file, flushed, and atomically replaced. On Unix, newly created SSH directories use mode `0700`, and managed files use mode `0600`. On Windows, managed files receive a non-inherited ACL granting access only to the current user.

Every successful mutation regenerates `sshls.conf`. If a machine or filesystem fails between the two atomic replacements, `sshls.toml` remains authoritative and `sshls config check` reports the stale fragment; `sshls config sync` repairs it.

The inventory schema is versioned and rejects unknown fields. Destination names are case-insensitively unique, output is sorted, and values that could inject OpenSSH directives are rejected.

`sshls resolve` delegates directly to `ssh -G`. `sshls test` invokes `ssh` without a shell, disables password prompts and connection sharing, refuses unknown host keys, disables host-key updates, and enforces both SSH and process-level timeouts. It never silently changes `known_hosts`.

## Inventory format

```toml
version = 1

[[hosts]]
name = "prod-web"
hostname = "203.0.113.10"
user = "deploy"
port = 22
identity_file = "~/.ssh/id_ed25519"
tags = ["prod", "web"]
```

The supported fields are intentionally limited to `name`, `hostname`, `user`, `port`, `identity_file`, and `tags`. Identity paths must be absolute or begin with `~/`, so they do not change meaning with the caller's working directory. Put shared or advanced options such as `ProxyJump`, forwarding, keepalives, and algorithms in your ordinary SSH config.

## Diagnostics and isolation

`sshls config check` validates the inventory, generated fragment, global include, identity files, installed SSH client, and effective OpenSSH resolution. It performs no network connection.

For isolated testing or portable setups:

- `SSHLS_DIR` changes the directory containing the inventory, generated fragment, and lock. It must be an absolute path.
- `SSHLS_USER_CONFIG` changes the user config inspected for the include. It must be an absolute path.
- `SSHLS_SSH_BIN` changes the SSH executable invoked by `resolve`, `test`, and `config check`.

These overrides are also what keep the test suite independent of a developer's real SSH setup.

## Deliberate exclusions

There is no SSH implementation, database, daemon, TUI, secret storage, key generation, arbitrary option passthrough, wildcard host management, config importer, grouping layer, or export format beyond the human-readable TOML source file. This narrow boundary is the principal maintenance feature.
