# Security policy

## Supported versions

Only the latest release gets security fixes. Cantiva is at 0.1.x.

## Reporting a vulnerability

Don't open a public issue. Use GitHub's private reporting instead: go to the repository's **Security** tab and choose **Report a vulnerability**. Include the version, what an attacker can do, and the steps to reproduce it.

You'll get an acknowledgement once the report has been read, and a fix or a decision will be published in [CHANGELOG.md](CHANGELOG.md) under **Security**.

## What counts

Cantiva runs local services and can expose an MCP server to the internet through a tunnel, so these are in scope:

- Reaching the control API (`127.0.0.1`), a worker or `cantiva-mcp` without the right token or secret, or from another machine.
- Getting past the tunnel restrictions: creating voices, playing audio, loading or unloading models, or reading local paths or transcripts from a tunnel-origin request.
- Creating a cloned voice without a consent record.
- Reading or writing files outside the Cantiva data folder or the output folder the user chose.
- Leaking the worker token, the API token, `mcp-secret` or the tunnel secret (for example into logs or command lines).

Out of scope: vulnerabilities in the TTS models, their pretrained weights or their upstream Python packages. Report those to the upstream projects listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Design notes

The local services bind to 127.0.0.1 and require a token. The tunnel is off by default; each time it opens it gets a new one-time secret, and tunnel requests run with a reduced tool set. Details are in [docs/architecture.md](docs/architecture.md#tunnel).
