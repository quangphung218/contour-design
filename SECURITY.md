# Security policy

## Scope

Contour is local-first. It has no accounts, no cloud service and no
telemetry. Its attack surface is the local machine:

- The daemon binds to `127.0.0.1` by default. Binding to another address
  requires `OD_API_TOKEN`, and a middleware rejects requests from origins
  other than the daemon's own (`OD_ALLOWED_ORIGINS` to add more).
- The desktop app runs the daemon in its own process on a random loopback
  port and grants Tauri IPC only to that origin.
- Previews run in a sandboxed iframe with a restrictive CSP.
- Coding agents run with the project folder as their working directory. In
  the default **auto** permission mode they approve their own commands, as
  in the original app; switch to **ask** in Settings ▸ Permissions to
  restrict them (see [docs/decisions.md](docs/decisions.md) §1).
- API keys (BYOK, media providers, MCP secrets) are stored in files with
  mode `0600` under the data folder and are never returned by the API.

Out of scope: what an agent you installed does with its own permissions,
and the security of the providers you configure.

## Supported versions

Only the latest release receives fixes.

## Reporting a vulnerability

Report privately. Do not open a public issue.

- Use GitHub's **Report a vulnerability** button on the repository's
  Security tab (private vulnerability reporting), or
- email the maintainer listed on the GitHub profile of `quangphung218`.

Include the version (`od --version`), the OS, the steps to reproduce and
what an attacker gains. Remove real API keys from logs.

You will get an acknowledgement within 7 days and a fix or a plan within
30 days. Credit is given in the release notes unless you prefer otherwise.
