# Contributing To Synapse

Thanks for contributing. This document is optimized for external developers who want to get from clone to first PR with as little guesswork as possible.

## Before You Start

- Read [README.md](README.md) or [README.zh-CN.md](README.zh-CN.md).
- Review [SECURITY.md](SECURITY.md) before reporting security-sensitive issues.
- Follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) in all project spaces.

## Development Environment

Synapse development currently targets Linux for the secure sandbox path.

Recommended local prerequisites:

- Rust stable
- `bwrap`
- `strace`
- cgroup v2 with writable `cpu`, `memory`, and `pids`
- `python3` for the default preview runtime flow

Validate your host:

```bash
cargo run -p synapse-cli -- doctor
```

Import a local Python runtime for testing:

```bash
cargo run -p synapse-cli -- runtime import-host --language python --version system --command python3 --activate
```

## Repository Layout

- `crates/synapse-core`: domain types, runtime management, sandboxing, audit, scheduler
- `crates/synapse-api`: axum HTTP surface
- `crates/synapse-cli`: CLI entrypoint and diagnostics
- `docs/`: product, design, quickstart, and release documentation
- `scripts/`: project gate and smoke scripts

## Typical Workflow

1. Create a branch for one logical change.
2. Make the smallest change that solves the problem.
3. Add or update tests for behavior changes.
4. Update docs if the public workflow changed.
5. Run the local gates.
6. Open a pull request using the repository template.

## Local Quality Bar

Run these from the repository root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/quickstart_smoke.sh
```

Notes:

- `scripts/quickstart_smoke.sh` verifies the documented quickstart path: runtime import, doctor, server startup, `health`, `execute`, `audits`, and `metrics`.
- If your host cannot satisfy Linux sandbox prerequisites, explain that in the PR and include the exact failing check.

## Documentation Expectations

If you change the public developer workflow, update all affected docs together:

- `README.md`
- `README.zh-CN.md`
- `docs/quickstart/10-minute-quickstart.md`
- `docs/api-reference.md`

The English README is the source-of-truth entry point. The Chinese README should mirror its structure and examples.

## Pull Requests

Open focused PRs with:

- what changed
- why it changed
- how you tested it
- any host assumptions or limitations

The repo includes a PR template in `.github/pull_request_template.md`. Use it instead of a free-form description so reviewers can quickly verify scope, tests, and doc impact.

## Commit Guidance

- Use concise imperative subjects.
- Keep each commit scoped to one logical change.
- Avoid mixing refactors with unrelated docs or behavior changes.

Examples:

- `docs: add bilingual quickstart and api reference`
- `ci: add quickstart smoke test`
- `cli: publish synapse binary name`

## Security-Sensitive Changes

For changes that touch sandboxing, auth, tenancy, audit logging, runtime integrity, or host interaction:

- call the risk out explicitly in the PR
- add regression coverage where possible
- avoid posting exploit details publicly if the issue is not yet fixed

## Questions

If the docs are unclear, open an issue or PR that improves them. For an early-stage project, documentation gaps are product gaps.
