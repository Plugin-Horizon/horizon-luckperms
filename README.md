# horizon-luckperms

> **Unofficial Rust port of LuckPerms for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the LuckPerms authors.**

LuckPerms-compatible permissions for [Horizon](https://github.com/arismoko/horizon) servers, as a native mod.
It reads and writes **LuckPerms' own MariaDB/MySQL tables** and speaks its **SQL messaging service**, so a Horizon
server shares ranks, permissions, prefixes and meta with Paper servers and a Velocity proxy that run LuckPerms
on the same database.

Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT). Not affiliated with LuckPerms or its authors.

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

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
does not exist yet (the Known gaps section). Until then `grant-via-execute = true` re-runs such a command from the console as
`execute as <player> at <player> run <command>`; its answer then goes to the server log. Off by default.

## Config

`config/luckperms.toml`, written with defaults on first start (the server then waits for the database password).
Keys follow LuckPerms' `config.yml`; see `data/luckperms/luckperms.example.toml`. Saving the file reloads it within a
second; storage settings need a restart. To share data with existing LuckPerms servers use the same database,
`table-prefix` and `messaging-service`, and give this server the `server` name its contexts should use.

## Web editor

`/lp editor [all|online|users|groups] [filter]`, `/lp user|group <x> editor`, `/lp track <x> editor` and
`/lp applyedits <code> [--force]` work as in LuckPerms: the groups, users and tracks are gzip-uploaded to bytebin
(`bytebin-url`, default `https://usercontent.luckperms.net/`) and the link to `web-editor-url` is printed; `applyedits`
downloads the reply and applies only what the editor changed. Only names, UUIDs, nodes, tracks and contexts are
uploaded, never IP addresses (an upload whose data looks like an IP address is refused). The live socket (bytesocks)
is not ported, so the editor shows the `/lp applyedits <code>` command to copy; `/lp trusteditor` answers that there is
no socket. Network calls run on the mod's worker thread (15 s timeout), never the tick.

## Verbose, tree, bulk update

- `/lp verbose on|record [filter]`, `off`, `upload|paste`: records the node checks the permission handler answers. The filter is
  LuckPerms' expression (`user`, a permission prefix, `true`/`false`/`undefined`, with `&`, `|`, `!` and brackets). Notifications
  go to the sender (50 a second for a player, 100 for the console); `upload` posts the results to bytebin and prints the link
  to `verbose-viewer-url`. There are no stack traces on Horizon. `verbose command` is not available. Only checks that reach the
  handler are seen, so set `permission-handler=luckperms:permissions`.
- `/lp tree [scope] [player]`: the tree of the known nodes (vanilla command nodes, `command-nodes`, LuckPerms' own and every node
  a mod asked about), uploaded to bytebin and shown at `tree-viewer-url`; with an online player the viewer also shows what
  they hold.
- `/lp bulkupdate <all|users|groups> <delete | update <permission|server|world> <value>> ["<field> <==|!=|~~|!~> <value>"]... [-s]`:
  console only, confirmed with `/lp bulkupdate confirm <id>` unless `skip-bulkupdate-confirmation` is set; `disable-bulkupdate`
  turns it off. It runs over the groups in memory and the users found in storage and saves each holder that changed.

## Export and import

- `/lp export [file] [--without-users] [--without-groups]`: writes LuckPerms' export document (gzip JSON: `metadata`, `groups`,
  `tracks`, `users`) to `config/luckperms/<file>.json.gz` (default `luckperms-yyyy-MM-dd-HH-mm.json.gz`, UTC). An existing file is
  not overwritten. `--upload` posts the document to bytebin and prints the code, as the web editor does.
- `/lp import <file> [--replace]` reads such a file from the same directory (the `.json.gz` ending may be left out). Groups and
  tracks are created when missing; users get their username and primary group. Without `--replace` the nodes are merged, with it
  each holder's nodes are replaced. `/lp import <code> --upload` reads a bytebin upload, including the web editor's reply.
- Only direct children of the data directory are used. Import nodes that already expired are dropped; malformed nodes are
  counted and reported, not applied.
- The file is written in the server's UTC clock and without the progress lines of LuckPerms' background threads (the work runs
  on the mod's worker thread in one go).

## Not ported

`translations` is listed like in LuckPerms and answers that it is not available on Horizon.
Also not ported: storage types other than MariaDB/MySQL (and `memory`), messaging other than SQL, Bukkit child permissions,
Vault and the PlaceholderAPI expansion. Details in the Known gaps section.

## Build

Build it as a native trait mod with arismoko's launcher (see the "native mods" how-to in the arismoko/horizon docs):

```sh
horizon-launcher native-build <path to this folder> --server-output <dir> --version <tag> --trust-native-code
```

`--trust-native-code` is your consent to compile and run this code inside the server. The logic crate tests run without the engine: `(cd core && cargo test)`.

## Permission handler

This mod provides the permission handler `luckperms:permissions`. Set `permission-handler=luckperms:permissions` in
`server.properties` so every mod's node queries are answered from LuckPerms; without it the built-in handler
answers from node defaults and `/lp` only edits the database. Nodes with no LuckPerms rule fall back to the
operator check (`op-level`, `enable-ops`) for online players; a node declared for everyone (level 0) by another mod
therefore needs an explicit grant on the `default` group, unless the mod is built with the cargo feature `horizon_handler_default` (off by default; it needs the engine change horizon-rust/horizon#18755): the handler then falls back to the default the owning mod declared for the node (everyone, operators of a level, nobody), as Paper falls back to plugin.yml. Offline UUIDs are refused (`UnknownPlayer`), never granted.

## Known gaps

What does not work yet on arismoko/horizon (each is marked `TODO(arismoko-api: ...)` in `src/lib.rs`):

- A granted vanilla command (`minecraft.command.<name>`) is lifted to run for a non-operator, but the command tree of a non-operator still hides that root.
- A login waits up to `horizon.login-wait-ms` (default 3000) for the player's data and is refused with a reason when the load fails or is late. With `0` the older behaviour returns: the player is let in with `default` rights and kicked after 5 seconds on failure. Neither path was exercised with a real database.
- `/lp verbose command` (the `verbose.command.others` node) needs running a command as a player and waiting for it: no engine call yet (GAPS_UPSTREAM).
- Storage types h2, sqlite, postgresql, mongodb, yaml, json, hocon, yaml-combined and split storage are refused at start; only MariaDB/MySQL and `memory` run.
- Messaging other than the SQL messenger (redis, nats, rabbitmq, pluginmsg) is refused; `messaging-service` must be `sql` or `auto` for an SQL network.
- `/lp translations` (listing and `install`) is not ported: the messages are English only.
- `require-sender-group-membership-to-modify` is read but not enforced: a sender may change a group they are not in.
- `argument-based-command-permissions`, `meta-value-selection` (and `meta-value-selection-default` other than `inheritance`), `apply-default-negated-permissions-before-wildcards`, `resolve-command-selectors`, `auto-op` (Bukkit's op flag does not exist here) are read and refused or noted at start, not applied.
- `apply-bukkit-default-permissions` and Bukkit child permissions (`plugin.yml` `children`): a declared node has no children on Horizon.
- Vault, the PlaceholderAPI expansion and the live web-editor socket (bytesocks; `/lp editor` gives the `applyedits` command instead) are not ported.
- The handler's answer for a node with no LuckPerms rule uses the node's declared default only with the cargo feature `horizon_handler_default` (needs horizon-rust/horizon#18755) and the `Nobody` default needs horizon-rust/horizon#18754.

## Licence and origin

MIT, see `LICENSE` and `NOTICE`. Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT, Copyright (c) lucko
(Luck) and contributors): its source was read and ported 1:1 for the command system, the schema, the message formats and
the resolution rules; its notice is kept in `LICENSE` and `NOTICE`. The message texts and the command specs are generated
from LuckPerms' own resource file and `CommandSpec`; the Rust around them is new.
