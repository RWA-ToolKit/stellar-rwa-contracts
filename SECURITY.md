# Security Policy

These contracts custody real-world asset value. Please report vulnerabilities responsibly.

## Reporting a Vulnerability

**Do not open a public issue.** Use [GitHub private vulnerability reporting](https://github.com/RWA-ToolKit/stellar-rwa-contracts/security/advisories/new)
to send a confidential report to the repository maintainers. Repository
administrators must keep private vulnerability reporting enabled for this
channel to accept reports.

Include:
- Description of the vulnerability
- Steps to reproduce
- Impact assessment
- Your contact information

## Response Timeline

These are targets, not guarantees. The clock starts when the report is submitted
through the private channel above.

| Stage | Target |
|-------|--------|
| Initial response (acknowledgement that a maintainer has seen the report) | within **48 hours** |
| Initial assessment: severity, whether it is accepted, and a fix timeline | within **7 days** |
| Patch for critical severity | within **30 days** |
| Public disclosure, coordinated with the reporter | within **90 days** |

If you have not received an initial response within the target, please add a
comment on your advisory to ping the maintainers again.

## Scope

- Smart contracts in `contracts/`
- Documentation in `docs/`
- Fuzzing targets in `fuzz/`
- Deployment scripts in `scripts/`
- Deployed contract instances listed in [`DEPLOYMENTS.md`](DEPLOYMENTS.md), including the named Testnet contracts. No Mainnet contract ids are currently published.

## Supported Versions

Security fixes are made on the default branch and released from it. There are
no long-lived release branches and no tagged releases yet, so only the latest
source receives fixes.

| Version / branch | Supported for security fixes |
|------------------|------------------------------|
| `main` (latest source) | Yes |
| Any other branch, fork or older commit | No: please reproduce against `main` first |
| Deployed contract instances in [`DEPLOYMENTS.md`](DEPLOYMENTS.md) | In scope for reports (see below); a fix means a new deployment of the patched source |

All deployed contract ids listed in [`DEPLOYMENTS.md`](DEPLOYMENTS.md) are in
scope, including an instance whose deployed version predates the current source
version. The current source `VERSION` constants are: asset-token `1`, compliance
`2`, dividend `5`, and registry `2`. These values are exposed by each contract's
`version()` function.

## Out of Scope

- Third-party dependencies (report to their maintainers)
- Social engineering or phishing
- Denial of service attacks on testnet

## Safe Harbor

We consider good-faith security research that follows this policy to be authorized and will not pursue legal action.
