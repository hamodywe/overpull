# Security Policy

## Supported versions

The latest released version receives security fixes.

## Reporting a vulnerability

Report privately through
[GitHub Security Advisories](https://github.com/hamodywe/overpull/security/advisories/new).
Please do not open a public issue for a vulnerability.

Include what you did, what happened, and what you expected. A reproduction —
a repository or a single file that triggers the behaviour — is the most
useful thing you can send.

You can expect an acknowledgement within a week and a fix or a clear
explanation of why it is not one within thirty days.

## Threat model

overpull reads source files from a project you point it at, and it never
executes them. That project may be untrusted — a dependency you are
evaluating, a pull request from a stranger — so the input is treated as
hostile:

- **No execution.** Source is parsed, never run. There is no plugin system,
  no configuration file that can execute code, no shelling out.
- **No network.** overpull makes no network requests at all.
- **Terminal output is sanitized.** File names, import specifiers and binding
  names taken from the scanned project pass through a control-character
  filter before printing, so a crafted file name cannot inject escape
  sequences into your terminal.
- **Bounded work.** Graph traversal is iterative rather than recursive, so a
  deeply nested or pathological import graph cannot blow the stack.
  Entry-order simulation is bounded.
- **Read-only.** overpull writes nothing except its own report to stdout.

### What would be a vulnerability

- A crafted source file that makes overpull execute code, write files, or
  make a network request.
- A crafted file name or specifier that escapes the output sanitizer.
- Input that causes unbounded memory growth or a hang rather than a bounded
  analysis.
- A stack overflow from a crafted import graph.

### What would not be

- An incorrect analysis verdict. That is a correctness bug — please open a
  regular issue, they are taken seriously.
- A panic on a file that is not valid source in any language overpull claims
  to read. Report it as a bug; it is not a security issue on its own.
