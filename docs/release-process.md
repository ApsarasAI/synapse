# Release Process

Synapse uses GitHub Releases as the official distribution channel for the developer preview.

## Versioning And Stability

- Versioning follows SemVer.
- While Synapse remains in `0.x`, minor releases may contain breaking changes.
- Every release should state clearly whether a change affects CLI behavior, API contracts, runtime packaging, or host requirements.

## Expected Release Artifacts

Each GitHub Release should attach:

- `synapse-linux-x86_64.tar.gz`
- `synapse-linux-x86_64.sha256`
- release notes with a concise change summary

The tarball should contain the `synapse` binary built from `crates/synapse-cli`.

## Release Checklist

1. Verify the workspace gates pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/quickstart_smoke.sh
```

2. Confirm the README, Chinese mirror, quickstart, and API reference still match the release behavior.
3. Tag the release as `vX.Y.Z`.
4. Let GitHub Actions build the Linux artifact and checksum.
5. Publish release notes with:
   - highlights
   - compatibility notes
   - breaking changes
   - upgrade guidance

## Binary Verification

End users should verify the checksum before using a release binary:

```bash
sha256sum -c synapse-linux-x86_64.sha256
```

## Preview Caveat

Release binaries are meant for developer evaluation, not production SLA commitments.
