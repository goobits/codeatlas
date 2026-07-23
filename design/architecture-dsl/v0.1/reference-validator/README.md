# Private Reference Validator

This unpublished crate is executable specification evidence for the proposed
Atlas Architecture DSL v0.1.

It is intentionally isolated:

- `publish = false`;
- no production Code Atlas workspace membership;
- no production dependency edge;
- no root Code Atlas CLI command;
- no stable public API guarantee;
- no network resolution;
- no architecture mutation;
- no Goobits or Workshop integration;
- its own lockfile and target directory.

Run its checks from the design-package root:

```sh
cargo test --locked --manifest-path reference-validator/Cargo.toml
```

Passing tests prove only that the proposed specification package is internally
consistent. They do not accept the proposal or authorize production use.
