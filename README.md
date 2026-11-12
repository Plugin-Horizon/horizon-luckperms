> **Frozen branch (`horizon-plain`).** This is the version written for horizon-rust/horizon's native hooks / horizon-mixins. We stopped working on it on 2026-11-12: horizon-rust moved all mod API work to the fork [arismoko/horizon](https://github.com/arismoko/horizon) (see "Mod-related pull requests" in its CONTRIBUTING.md) and closed mod pull requests there. This branch is experimental and unsupported. The maintained version, for arismoko/horizon only, is on the default branch.

# horizon-luckperms

> **Unofficial Rust port of LuckPerms for the Horizon server (https://github.com/horizon-rust/horizon). Not affiliated with or endorsed by the LuckPerms authors.**

LuckPerms-compatible permissions for [Horizon](https://github.com/horizon-rust/horizon) servers, as a native mod.
It reads and writes **LuckPerms' own MariaDB/MySQL tables** and speaks its **SQL messaging service**, so a Horizon
server shares ranks, permissions, prefixes and meta with Paper servers and a Velocity proxy that run LuckPerms
on the same database.

Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT). Not affiliated with LuckPerms or its authors.

**State: blind port.** Written against Horizon `a589a3dd9` without an engine build or a running server; the core
crate is unit-tested on its own. See [STATUS.md](STATUS.md) for what is done and [GAPS.md](GAPS.md) for what
Horizon still needs.

## What it does

- **Storage**: LuckPerms' schema (`{prefix}user_permissions`, `group_permissions`, `players`, `groups`, `tracks`,
  `actions`), created when missing, with the same column formats (`server`/`world` columns, `contexts` JSON,
  `expiry` in unix seconds). Changes are saved as differences, as LuckPerms does.
- **Messaging**: the `{prefix}messenger` table, polled every second; `update`, `userupdate` and `log` messages in
  LuckPerms' JSON. A change made here pings the other servers; a ping from them reloads the data here.
- **Resolution**: groups, inheritance (weights, the three traversal algorithms), contexts (`server`, `world`,
  `dimension-type`, `gamemode`, static contexts), wildcards, regex nodes, shorthand, temporary nodes, prefix and
  suffix stacks, meta, primary group: LuckPerms' rules (see [Licence and origin](#licence-and-origin)).
- **`/lp`** (also `/luckperms`, `/perm`, `/perms`, `/permission`, `/permissions`): `user`, `group` and `track`
  with their `permission`, `parent` and `meta` sub commands, `promote`/`demote`, `listgroups`, `creategroup`,
  `deletegroup`, track commands, `search`, `log`, `info`, `sync`, `networksync`, with LuckPerms' permission
  names (`luckperms.user.permission.set`, ...) and action-log descriptions. The console may use all of it.
- **Commands of the game**: a typed command is checked against `minecraft.command.<name>` (Paper's names for
  vanilla commands) or a node from `[horizon.command-nodes]`; a node set to false refuses it, also for operators,
  and leaves it out of the player's command tree.
- **For other mods**: `horizon_luckperms_core::api` (`has_permission`, `check`, `prefix`, `suffix`, `meta_value`,
  `primary_group`, `groups`), read from a cache, never from the database.

### Operators

Horizon has operator levels (0 to 4), not Bukkit's op flag. A player at `op-level` (default 1) or above counts
as op: every permission not explicitly set false is true for them (LuckPerms `enable-ops`), and they may use
`/lp` without `luckperms.*` nodes (`commands-allow-op`).

Granting a command above a player's level (a non-op with `minecraft.command.gamemode`) needs a Horizon hook that
does not exist yet (GAPS.md). Until then `grant-via-execute = true` re-runs such a command from the console as
`execute as <player> at <player> run <command>`; its answer then goes to the server log. Off by default.

## Config

`config/luckperms.toml`, written with defaults on first start (the server then waits for the database password).
Keys follow LuckPerms' `config.yml`; see `core/luckperms.example.toml`. Saving the file reloads it within a
second; storage settings need a restart. To share data with existing LuckPerms servers use the same database,
`table-prefix` and `messaging-service`, and give this server the `server` name its contexts should use.

## Build

A native mod bundle (`horizon-mixin.toml`): the `core/` crate plus HEAD injections into Horizon's hook points
(helpers in `glue/`). Build it with the Horizon launcher (`horizon-launcher native-build <this folder> ...
--trust-native-code`) or as the launcher does: `horizon-mixins apply` on an exported Horizon at the pinned
revision, then build `horizon-client`.

The core crate alone: `cd core && cargo test --no-default-features` (no MariaDB driver) or `cargo test`.

## Web editor

`/lp editor [all|online|users|groups] [filter]`, `/lp user|group <x> editor`, `/lp track <x> editor` and
`/lp applyedits <code> [--force]` work as in LuckPerms: the groups, users and tracks are gzip-uploaded to bytebin
(`bytebin-url`, default `https://usercontent.luckperms.net/`) and the link to `web-editor-url` is printed; `applyedits`
downloads the reply and applies only what the editor changed. Only names, UUIDs, nodes, tracks and contexts are
uploaded, never IP addresses (an upload whose data looks like an IP address is refused). The live socket (bytesocks)
is not ported, so the editor shows the `/lp applyedits <code>` command to copy; `/lp trusteditor` answers that there is
no socket. Network calls run on the mod's worker thread (15 s timeout), never the tick.

## Not ported

`verbose`, `tree`, `bulkupdate`, `import`, `export` and `translations` are listed like in LuckPerms and answer
that they are not available on Horizon.

`verbose`, `tree`, `bulkupdate`, `import`/`export`, translations,
storage types other than MariaDB/MySQL, messaging other than SQL, Bukkit child and default permissions. Details in
[STATUS.md](STATUS.md).

## Licence and origin

MIT, see `LICENSE` and `NOTICE`. Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT, Copyright (c) lucko
(Luck) and contributors): its source was read and ported 1:1 for the command system, the schema, the message formats and
the resolution rules; its notice is kept in `LICENSE` and `NOTICE`. The message texts and the command specs are generated
from LuckPerms' own resource file and `CommandSpec`; the Rust around them is new.

## Contributing

Pull requests and improvements are always welcome! Open an issue or a PR -- direct pushes to main are disabled, every change goes through review.
