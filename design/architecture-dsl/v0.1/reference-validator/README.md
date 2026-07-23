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
cargo fmt --manifest-path reference-validator/Cargo.toml -- --check
cargo test --locked --jobs 1 --manifest-path reference-validator/Cargo.toml
cargo clippy --locked --jobs 1 \
  --manifest-path reference-validator/Cargo.toml \
  --all-targets -- -D warnings
cargo run --locked --jobs 1 \
  --manifest-path reference-validator/Cargo.toml \
  --bin generate_artifacts -- --check
```

Passing tests prove only that the proposed specification package is internally
consistent. They do not accept the proposal or authorize production use.

To refresh the committed generated observation, conformance report, and stable
manifest after changing an input:

```sh
cargo run --locked --jobs 1 \
  --manifest-path reference-validator/Cargo.toml \
  --bin generate_artifacts -- --write
```

Generated files identify the generator, source inputs, and generation command.
Do not edit them by hand.
