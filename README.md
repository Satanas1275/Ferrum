# Homemade Minecraft Server (Rust)

A Minecraft server written from scratch in Rust (tokio, async), with no dependency on an existing protocol library. Networking, world generation, player state, inventory, etc. are all implemented by hand.

Each major Minecraft version changes the protocol, block/mob registries, and world generation enough to justify a separate implementation. This repo therefore follows a **one branch per supported version** model rather than a single shared codebase across versions.

## Available branches

| Branch | MC version(s) | Status |
|---|---|---|
| [`1.7`](../../tree/1.7) | 1.7.10 | ✅ Active |

More branches will be added as new versions are supported (see Roadmap).

This `main` branch contains no code — it's an entry point / showcase for the project only.

## Why one branch per version?

- The network protocol (packet IDs, data layout) changes between major versions.
- New blocks and mobs added each version require an updated whitelist on the security side (e.g. rejecting block placement for anything the server doesn't recognize).
- World generation changes fairly often.
- As of recent versions (26.x), Mojang no longer obfuscates the Java client, which changes the implementation approach again.

Sharing a single codebase across all versions would require a translation layer similar to ViaVersion — out of scope for now.

## Running a version

```bash
git checkout 1.7
cargo build --release
./target/release/<binary_name> --default-config
```

(Always use `--release`: debug mode is noticeably slower, especially for chunk generation/compression.)

## Contributing / porting a fix across branches

Generic gameplay bugs (physics, inventory, respawn, etc.) often affect several branches at once. Convention followed here:

1. The fix is committed first on the branch where the bug was found.
2. It's then ported to other active branches via `git cherry-pick <hash>`.
3. Each branch keeps a `CHANGELOG.md` to track which gameplay fixes have already been ported, to avoid missing or duplicating them across versions.

## Roadmap

- [x] 1.7.10
- [ ] 1.8.x
- [ ] 1.12.x
- [ ] 1.16.x
- [ ] 1.20.x / 1.21.x
- [ ] Track 26.x versions (non-obfuscated client)

## License

Licensed under **CC BY-NC-SA 4.0** (Attribution-NonCommercial-ShareAlike). In short:

- ✅ Forking, modifying, and redistributing is allowed
- ✅ Publishing your fork's source is encouraged but not required
- ❌ Commercial use (including selling the server or a modified version) is not allowed
- 🔁 If you redistribute a modified version, it must stay under the same license

See [LICENSE](LICENSE) for the full text.