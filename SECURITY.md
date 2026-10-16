# Security Policy

## Supported Versions

Only the latest released version of EVE Traveling Merchant is actively supported with security updates.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1.0 | :x:                |

## Security Architecture & Credentials Policy

EVE Traveling Merchant adheres to strict zero-secret and local-isolation design principles:

1. **Zero Compiled Secrets**: No private API secrets, developer passwords, or client secrets are compiled into application binaries or committed to version control.
2. **RFC 7636 PKCE Authentication**: The application implements OAuth 2.0 Proof Key for Code Exchange (PKCE) for public desktop clients. Ephemeral cryptographic verifiers and SHA-256 challenges are generated per session via system CSPRNG (`ring`) and never logged.
3. **Local Token Storage & Permissions**:
   - On Linux/Unix, character tokens are stored at `~/.config/eve-traveling-merchant/sso_token.json` with POSIX `0600` file permissions and `0700` parent directory permissions.
   - On Windows, tokens are stored within the protected user profile at `%APPDATA%\eve-traveling-merchant\sso_token.json`.
4. **Credential Redaction**: Access tokens, refresh tokens, and code verifiers are masked as `[REDACTED]` in debug representations and logs.
5. **Minimal Scope Footprint**: Only read-only location/ship identification and waypoint insertion scopes are requested. The application never requests wallet, assets, trade execution, or EveMail permissions.

## Reporting a Vulnerability

If you discover a security issue or vulnerability in EVE Traveling Merchant:

1. **Do not create a public GitHub issue.**
2. Send a report privately via GitHub Security Advisories or email the maintainer at `28274575+Flambello@users.noreply.github.com`.
3. Include detailed steps to reproduce the issue and any potential impact.

We will acknowledge receipt within 48 hours and work with you on a resolution prior to any public disclosure.
