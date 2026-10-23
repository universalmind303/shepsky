# Security boundaries

The bridge defaults to `127.0.0.1:7717`. Keep it behind your tailnet and treat
the bearer token as authority over the connected herdr session. A token
holder can read visible terminal contents from any known pane, including
plain shells, read supported agent transcripts, and submit prompts or keys
through herdr's agent methods. Coding agents can perform powerful actions;
the bridge is not an operating-system sandbox.

## Temporarily unavailable operations

Phone start, rename, and close requests fail before any RPC is sent. The
herdr allowlist independently excludes creation, lifecycle mutations,
rollback closes, and raw pane input. The protocol variants and planning
code remain so older clients receive an explicit error rather than an
unrecognized-message failure. They must not be re-enabled merely by adding
another bridge-side check.

Re-enabling lifecycle operations requires herdr to bind the expected agent
identity to the actual mutation. Starting also needs herdr to bind a checked
directory handle to directory use. Omitted start directories are rejected
by the planner as an additional safeguard. `serve --demo` and the demo
startup helper fail before connecting to herdr or loading a pairing token;
an independently constructed demo handler also refuses input.

## Resource and credential limits

- Four WebSocket connections total, including unauthenticated clients;
  64 KiB incoming frames/messages and a 1 KiB auth envelope.
- Authentication expires after ten seconds. Stalled outgoing writes expire
  after ten seconds. Connections exceeding 32 text requests in a one-second
  window are closed.
- One mutation runs at a time per connection, with no detached mutation
  tasks. A request already accepted can finish if the phone disconnects;
  queued requests are not spawned as background work. Herdr RPCs retain
  their ten-second timeout and now cap response lines at 8 MiB.
- Eight pane watches and four chat watches per connection; the connection
  cap also bounds aggregate watches. Outgoing queues hold eight messages.
- Transcript stitching retains at most 512 items and approximately 2 MiB
  of item data, plus up to 128 bounded unmatched results. Evicted tool calls
  may no longer receive completion updates. Paging still returns the loaded
  page. Individual transcript lines are capped at 8 MiB.

Token comparisons use `subtle`. Dependency `log` debug/trace calls are
compiled out of bridge builds, so `RUST_LOG=trace` cannot enable tungstenite
frame/auth payload logging. Invalid herdr responses are reported without
embedding the response body. The explicit `pair` command still intentionally
prints the pairing credential; treat its output as secret.

Android pairing preferences are excluded from both cloud backup and device
transfer, and application backup is disabled. Cleartext `ws://` remains
supported intentionally; use tailnet transport or `wss://` for remote access.

## Transcript files

Transcript opens walk absolute paths one component at a time using
descriptor-relative, no-follow opens. Only regular files under the selected
transcript root are accepted. The reader retains the opened descriptor;
later paging never reopens an unchecked pathname. Replacement or rotation
invalidates the watch and forces resolution again. Codex metadata/index and
candidate scoring reads use the same no-follow opening discipline.

This deliberately rejects symlinked transcript roots and paths. It does not
isolate the bridge from a process with full control of the same user account
or defend against malicious content deliberately written into an authorized
regular transcript file.

## Validation

Regression tests use synthetic tokens, temporary files, a fake herdr Unix
socket, and an ephemeral loopback listener. They cover auth gating, request
and connection limits, denied lifecycle RPCs, sequential mutations,
transcript replacement, no-follow opens, retained-memory limits, and the
compiled dependency logging level. No production socket or credentials are
needed for these tests.
