# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace (`Cargo.toml` at root) with three crates under `crates/`:
- `crates/synapse-core`: core domain types, error model, and execution abstractions.
- `crates/synapse-api`: HTTP API layer built with `axum` (`server.rs`).
- `crates/synapse-cli`: CLI entrypoint (`main.rs`), including `serve`.

Design and product notes live in `docs/design-docs/`. Security reporting guidance is in `SECURITY.md`.

## Build, Test, and Development Commands
Run commands from the repository root:
- `cargo check --workspace`: fast compile validation for all crates.
- `cargo build --workspace`: build all workspace members.
- `cargo test --workspace`: run all unit/integration tests.
- `cargo run -p synapse-cli -- serve --listen 127.0.0.1:8080`: start local API.
- `curl http://127.0.0.1:8080/health`: verify service health endpoint.

## Coding Style & Naming Conventions
- Rust edition: `2021` (workspace-level).
- Formatting: run `cargo fmt --all` before committing.
- Linting: run `cargo clippy --workspace --all-targets -- -D warnings`.
- Naming:
  - `snake_case` for functions/modules/files.
  - `PascalCase` for structs/enums/traits.
  - Keep crate names prefixed with `synapse-` for workspace consistency.

Keep modules focused and small; place shared domain logic in `synapse-core`, not in API/CLI glue code.

## Testing Guidelines
Use Rust’s built-in test framework (`cargo test`).
- Unit tests: colocate in `src/*` with `#[cfg(test)]`.
- Integration tests: place under each crate’s `tests/` directory.
- Prefer table-driven cases for request/validation logic and error handling.

No coverage gate is currently configured; add tests for all behavior changes and bug fixes.

## Commit & Pull Request Guidelines
Current history is minimal (single initial commit), so no strict commit format is enforced yet.
- Write concise, imperative commit subjects (example: `api: add health response schema`).
- Keep commits scoped to one logical change.

For pull requests:
- Explain what changed and why.
- Link related issues/docs when applicable.
- Include CLI/API usage examples for behavior changes (sample command or `curl`).
- Ensure `cargo fmt`, `cargo clippy`, and `cargo test` pass locally.
