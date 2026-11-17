# Security policy

## Reporting a vulnerability
Please **do not** open a public issue for security problems.

Use GitHub's private reporting instead. On this repository, open **Security → Report a vulnerability**. Only the maintainers can see the report.

Please include:
- what an attacker could do, and what they need (for example, local access, the same LAN, or only the public link);
- the steps to reproduce it, and the app version (shown at the bottom left of the app);
- logs, if they help. Exported logs already have passwords and tokens removed.

You should get a first answer within 7 days. A fix is released as soon as possible, and you are credited in the release notes unless you prefer otherwise.

## Supported versions
Only the latest release receives security fixes.

## Scope
**In scope:** the desktop app, the CLI, the web-server and database configuration they generate, how secrets are stored, how runtimes are downloaded and verified, and the hosting integrations.

**Out of scope:** bugs inside the websites or games you run with the app (for example, SQL injection in your own PHP code), and problems in the third-party programs themselves (PHP, MariaDB and so on). Please report those upstream.

What the app already protects against is described in [docs/SECURITY_REVIEW.md](docs/SECURITY_REVIEW.md).
