# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in Venus, please use [GitHub private vulnerability reporting](https://github.com/alvezgr/venus/security/advisories/new).
Do not include credentials in public issues.

Please include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes (if applicable)

Include the affected Venus version and whether the issue involves Insomnia Cloud Sync.

## Security Best Practices

Venus is designed with security in mind:
- No telemetry or data collection
- API requests and explicitly requested Insomnia authentication and cloud sync
- Local workspace and session storage; chosen changes are uploaded to Insomnia Cloud when you push
- No automatic updates without user consent
