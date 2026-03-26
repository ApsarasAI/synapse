# Security Policy

## Supported Versions

Synapse is currently released as a `0.x` developer preview.

Security fixes are applied on a best-effort basis to:

- the latest release on `main`
- the latest published GitHub Release in the current `0.x` line

Older preview builds may not receive backports.

## Reporting A Vulnerability

Do not open a public issue for suspected vulnerabilities.

Please report security issues privately to the maintainers through one of these channels:

- GitHub Security Advisories for this repository
- private email to the maintainers at `security@synapse.dev`

Include, when possible:

- affected version or commit SHA
- host environment details
- reproduction steps or proof of concept
- expected impact
- any mitigation you already tested

## Response Targets

Current best-effort response SLA:

- acknowledgment within 3 business days
- triage decision within 7 business days
- status update at least every 14 days for accepted reports

This is not a production SLA.

## Disclosure Process

1. The maintainers acknowledge receipt and assign an internal severity.
2. We reproduce and scope the issue.
3. A fix or mitigation is prepared privately when reasonable.
4. Coordinated disclosure timing is agreed with the reporter.
5. A public advisory, release note, or commit reference is published once users have a remediation path.

## Scope Notes

The current release target is Linux secure sandbox execution. Reports are especially helpful for:

- sandbox escapes
- audit log tampering or disclosure
- runtime integrity bypasses
- authentication and tenant isolation flaws
- metrics or logs leaking sensitive host data

Reports that depend on unsupported platforms may still be useful, but they are lower priority than Linux-path issues.
