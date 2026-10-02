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

- **48 hours**: Acknowledgement
- **7 days**: Initial assessment and timeline
- **30 days**: Target patch for critical severity
- **90 days**: Public disclosure coordination

## Scope

- Smart contracts in `contracts/`
- Documentation in `docs/`
- Fuzzing targets in `fuzz/`
- Deployment scripts in `scripts/`
- Deployed contract instances listed in [`DEPLOYMENTS.md`](DEPLOYMENTS.md), including the named Testnet contracts. No Mainnet contract ids are currently published.

## Supported Versions

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
