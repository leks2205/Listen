# Security policy

Listen is a desktop app. It keeps your server passwords, YouTube Music session and API keys on your own computer
(the system keyring, or a credentials file in Listen's config folder when no keyring is available) and only sends
them to the service they belong to. It has no backend of its own.

## Reporting a vulnerability

Please don't open a public issue for security problems. Report them privately through GitHub instead: open the
**Security** tab of this repository and click **Report a vulnerability**.

Helpful reports name the version, the platform (Linux or Windows), steps to reproduce and what an attacker could
achieve (for example reading credentials, or code execution through a crafted server answer, playlist or theme file).

Only the latest release receives fixes.
