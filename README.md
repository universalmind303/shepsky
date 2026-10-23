# collie

Watch and answer your [herdr](https://herdr.dev) coding agents from your phone.
A collie works the herd so the herder doesn't have to stand in the field.

- **`crates/collie-core`** — shared Rust: herdr wire types, session store,
  notification dedup. Pure logic, no I/O.
- **`crates/collie-bridge`** — Mac-side daemon: syncs with herdr's unix socket
  and (M2) serves phones over WebSocket, exposed tailnet-only via
  `tailscale serve`.
- **`android/`** — (M3) Kotlin + Jetpack Compose app calling a Rust cdylib
  built on `collie-core` through a minimal hand-written JNI surface.

Requires herdr ≥ 0.9.5. Protocol details and sharp edges: `docs/herdr-notes.md`.

## Status: M1

```sh
cargo run -p collie-bridge -- status          # one-shot agent table
cargo run -p collie-bridge -- status --watch  # live view (alt screen)
```

`--socket PATH` overrides herdr socket discovery
(`$HERDR_SOCKET_PATH`, then `~/.config/herdr/herdr.sock`).

## Roadmap

1. ~~M1 — core types + store + sync loop + `status --watch`~~
2. M2 — WebSocket server, bearer-token pairing, bridge↔phone protocol (`docs/PROTOCOL.md`)
3. M3 — Android app skeleton: pairing, overview, foreground service + notifications
4. M4 — prompt/approve from the phone, live terminal view
5. M5 — polish: mutes, RemoteInput reply, launchd, QR pairing
