# Minecraft Server — 1.7.10 (Rust)

A from-scratch async Minecraft server implementation for **1.7.10**, written in Rust with tokio. No existing protocol library used — packet handling, world state, and gameplay logic are all hand-rolled.

> ⚠️ **Early stage.** This is a personal/hobby project, not a hardened production server. See [Known limitations / security](#known-limitations--security) before exposing it to anyone you don't trust.

## Features (this version)

- **Protocol handling** — handshake, login, status/ping
- **Player management** — join/leave, movement, gamemode, chat, sneak
- **World management** — flat world generation, chunk data (with metadata), block place/break with validation
- **Block support cascade** — unsupported blocks break and drop items automatically
- **Interactive blocks** — doors, trapdoors, fence gates, levers, buttons, chests, furnaces, etc.
- **Full inventory system** — click, shift-click, hotbar swap, creative mode
- **Item entities** — drop, pickup with delay, despawn after 5 minutes
- **Food system** — eat to heal (apple, bread, meats, etc.)
- **Combat** — entity use (attack), fall damage, health system
- **Teleportation** — relative coordinates, cross-player, `@a` selector
- **Console** — tab completion, `help`, `list`, `say`, `stop`, `gamemode`, `tp`, `tps`, `load`
- **In-game commands** — `/gamemode`, `/tp`, `/tps`, `/load`, `/help`
- **TPS tracking** — dedicated tick thread at 20 TPS, CPU/RAM monitoring
- **Redstone** — wire, torches, repeaters, comparators, powered components (⚠️ experimental)
- **Placement rules** — surface-type checks (flowers on grass, cactus on sand, etc.)
- **Configuration** — via `server.json`
- Built on **Rust 2024 edition**, tokio async runtime (multi-threaded)

## Requirements

- Rust (2024 edition toolchain)
- A Minecraft 1.7.10 client to connect

## Running

```bash
cargo build --release
./target/release/<binary_name> --default-config
```

Always run with `--release` — debug builds are noticeably slower, especially chunk generation/compression.

`--default-config` generates a default `server.json` on first run if none exists. Edit it to change port, MOTD, max players, etc.

## Known limitations / security

This build has **no anti-cheat / server-side validation** yet. Known gaps:

- No duplicate-username protection — two clients can join with the same name.
- No real authentication — no Mojang/Microsoft online-mode login handling.
- ⚠️ **Redstone is experimental** — most complex circuits won't work correctly yet. Pistons don't extend/retract, some propagation edge cases are missing.
- Multithreading is not fully optimized yet, which may cause TPS drops under heavy load.

Don't run this on a public, untrusted network yet. Treat it as LAN/friends-only until the security pass (see roadmap) lands.

## Coming soon

- [x] Basic multithreading (dedicated tick thread, multi-worker async runtime)
- [x] Block placement validation and surface-type rules
- [x] Block support cascade (auto-break unsupported blocks)
- [x] Redstone wire, torches, repeaters, comparators (experimental)
- [ ] Fix redstone propagation edge cases
- [ ] Piston extension/retraction
- [ ] Real world generation (currently flat only)
- [ ] Sound implementation
- [ ] Small fixes (knockback on PvP, correct damage values)
- [ ] Critical hit implementation
- [ ] Ender chest
- [ ] Beds and sleeping
- [ ] Server-side security pass (duplicate username prevention)
- [ ] Mojang/Microsoft online-mode login
- [ ] Rust mod support
- [ ] Forge mod support
- [ ] Fabric mod support
- [ ] Ongoing bugfixing as issues are reported

## License

CC BY-NC-SA 4.0 — see [LICENSE](LICENSE) on the `main` branch. TL;DR: fork it, modify it, share it, just don't sell it, and keep derivatives under the same license.

## Contributing

Bug reports and PRs welcome. If a fix touches gameplay logic shared across versions, it may get cherry-picked to other version branches as they're added — see `main` for the branching convention.