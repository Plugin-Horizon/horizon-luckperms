# horizon-luckperms

## Port status

Compared with LuckPerms b878ae9 (master): **98 %** of the original's commands, permissions, config keys, messages and features work today; **98 %** with the combined next SDK; **99 %** once the Vault provider is ported. Counted on 262/263 items (audit item list rec100/luckperms.md; the open item is the Vault provider, which needs an engine API; 6 more lines are n/a with a reason). Status: round 4 written, build queued, not yet compiled.

> **Unofficial Rust port of LuckPerms for the Horizon server (https://github.com/arismoko/horizon). Not affiliated with or endorsed by the LuckPerms authors.**

LuckPerms-compatible permissions for [Horizon](https://github.com/arismoko/horizon) servers, as a native mod.
It reads and writes **LuckPerms' own MariaDB/MySQL tables** (or its SQLite file, or its YAML, JSON, HOCON and TOML folders) and speaks
its **SQL, Redis and NATS messaging services**, so a Horizon server shares ranks, permissions, prefixes and meta with Paper
servers and a Velocity proxy that run LuckPerms on the same database or broker.

Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT). Not affiliated with LuckPerms or its authors.

> **Built for and working only with [arismoko/horizon](https://github.com/arismoko/horizon)** (the Horizon fork that carries the mod API). [horizon-rust/horizon](https://github.com/horizon-rust/horizon) is experimental for mods and not supported: the earlier version for its native hooks / horizon-mixins is frozen on the `horizon-plain` branch (work stopped on 2026-11-12).

## Which Horizon

This is a native trait mod: `horizon-mod.toml`, a `Cargo.toml` that depends on `horizon-mod-api`, `src/lib.rs` with the hooks, and the plugin logic in the `core/` crate (plain Rust, no engine needed for its tests).

- Pin: `[native] horizon = "54090ee36c4e2c7b43b66db864220c577b94b7e0"` in `horizon-mod.toml` (game `26.3`). That is a development build of arismoko/horizon `main` plus mod API additions that are not all merged upstream yet, so a stock `main` may lack some host methods this mod calls until they land.
- It compiled and loaded on that build and was smoke-tested there; it has not seen a long run with real players. Treat it as experimental.
- Feature descriptions below were written for the earlier horizon-rust version; where they name engine calls (`native_*`) or old file paths, read them as descriptions only. The Known gaps list is current.

## What it does

- **Storage** (`storage-method`): `mariadb`/`mysql` with LuckPerms' schema (`{prefix}user_permissions`, `group_permissions`,
  `players`, `groups`, `tracks`, `actions`), created when missing, with the same column formats (`server`/`world` columns,
  `contexts` JSON, `expiry` in unix seconds); changes are saved as differences, as LuckPerms does. `sqlite` keeps the same
  tables in `luckperms-sqlite.db`. `yaml`, `json`, `hocon`, `toml` keep one file per user, group and track in
  `<format>-storage/users|groups|tracks/`, and the `-combined` variants one file per kind (`users.yml`, ...), in
  Configurate's layout, plus `uuidcache.txt` and `actions.txt`; a folder written by LuckPerms on Paper reads here. `split-storage`
  puts user, group, track, uuid and log data into different storages. The files live in the original's `plugins/LuckPerms/`
  when its `config.yml` is used, else in `config/luckperms/`. `memory` is for tests. `h2`, `postgresql` and `mongodb` cannot
  run (below).
- **Messaging** (`messaging-service`): `sql` polls the `{prefix}messenger` table every second (MariaDB/MySQL only); `redis`
  (plain, TLS, sentinel; a cluster through its first node) and `nats` (plain, TLS, user/password or token) carry the same
  messages on the channel `luckperms:update`; `auto` picks Redis, then NATS (when enabled), then SQL. Messages are LuckPerms'
  JSON (`update`, `userupdate`, `log`). A change made here pings the other servers; a ping from them reloads the data here.
  As in LuckPerms `none` means `auto`; `notsql` turns messaging off.
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
uploaded, never IP addresses (an upload whose data looks like an IP address is refused). Network calls run on the mod's
worker thread (15 s timeout), never the tick.

**The live socket** works as in LuckPerms: `/lp editor` also opens a channel on a bytesocks service (`bytesocks-url`, default
`https://usersockets.luckperms.net/`) and puts the channel and this server's public key into the request. The editor connects
to the channel; the first time a browser connects the player is told (with a click to trust it, in the console the command) and
runs `/lp trusteditor <id>`; from then on the editor sends its changes over the socket and `/lp applyedits <code>` is not
needed. Messages are signed with ECDSA P-256 (the editor's protocol version 2); a trusted browser is remembered by the hash of its
key, as a `lp-editor-key` meta node of the player (`editor-keystore.json` for the console). Only the browser that was trusted
may send changes (its signature is checked), a session can be applied once, and the sender needs `luckperms.applyedits`. An
unused socket is closed after a minute, an idle one after a minute without a ping. When the service cannot be reached the
editor shows the `/lp applyedits <code>` command, and `/lp trusteditor` says there is no socket.

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

Not ported: the storages `h2`, `postgresql`, `mongodb`; the messaging services `rabbitmq`, `postgresql`, `pluginmsg`; Vault.
Details in the Known gaps section.

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
therefore needs an explicit grant on the `default` group unless the handler can see the declared default: it falls back to the default the owning mod declared for the node (everyone, operators of a level, nobody), as Paper falls back to plugin.yml. Offline UUIDs are refused (`UnknownPlayer`), never granted.

## Known gaps

What does not work yet (round 4, 2026-11-25; the item list counts 262 of 263 done, six more lines are n/a with the reasons below):

- A granted vanilla command (`minecraft.command.<name>`) runs for a non-operator (the line is cancelled and run again from the next tick at the level it needs, with the player's own feedback) and its root is put back into the command tree the client is shown (`player_commands_sent_restore`), so it completes.
- A login waits up to `horizon.login-wait-ms` (default 3000) for the player's data and is refused with a reason when the load fails or is late. With `0` the older behaviour returns. Neither path was exercised with a real database.
- n/a, `storage-method: h2`: H2 is a Java embedded database; no Rust implementation of it exists among the crates a mod may use (`horizon-mod-deps`). Use `sqlite` (same schema), a file storage, `mariadb` or `mysql`. The split-storage methods default to `h2` in LuckPerms; name them.
- n/a, `storage-method: postgresql` and `mongodb`: `horizon-mod-deps` has no PostgreSQL or MongoDB client (a hand-written wire client with SCRAM would be a library of its own). The mod refuses to start with a message naming the method.
- n/a, `messaging-service: rabbitmq`: no AMQP client among the dependencies. `postgresql` (LISTEN/NOTIFY) needs the PostgreSQL client above. `pluginmsg` sends over a player's connection to a relay on the proxy; a mod's registered payload channels are negotiated with the client and a Velocity proxy does not take part, so it cannot work. Each is refused with a message at start and messaging stays off.
- Redis through a cluster talks to the first node only (pub/sub reaches every node of a cluster). The Redis, NATS, SQLite, file-storage and live-socket code is tested against stand-ins (a fake broker, an in-memory service, temp folders); none of it has run against a real broker or the real bytesocks service yet.
- Split storage with a MariaDB/MySQL part carries the SQL messenger on that part. A file storage or SQLite cannot carry it (`sql` messaging is then refused with a message).
- Vault is not ported. The service `luckperms:placeholder` (`call_mod_service`; payload `<uuid hex> <params>`, e.g. `prefix`, `meta_<key>`) answers the `%luckperms_*%` values for the PlaceholderAPI port. The primary group and group list of every online player are published to the engine (`permission_groups`) every half second for the mods that read them.
- Translations use the server language (`[horizon] locale`, or the environment variable `LUCKPERMS_LOCALE`), not each player's client language.

Done in round 2: `/lp verbose command` (and `luckperms.verbose.command.others`), `auto-op` (`luckperms.autoop`), `argument-based-command-permissions`, `require-sender-group-membership-to-modify`, `meta-value-selection(-default)`, `apply-default-negated-permissions-before-wildcards`, `/lp translations [install]` with `translations/<locale>.properties`, the node's declared default (`Nobody` included) in the handler (the cargo feature `horizon_handler_default` is gone), Bukkit child permissions (the engine resolves `add_with_children`).

## Licence and origin

MIT, see `LICENSE` and `NOTICE`. Derived from LuckPerms (https://github.com/LuckPerms/LuckPerms, MIT, Copyright (c) lucko
(Luck) and contributors): its source was read and ported 1:1 for the command system, the schema, the message formats and
the resolution rules; its notice is kept in `LICENSE` and `NOTICE`. The message texts and the command specs are generated
from LuckPerms' own resource file and `CommandSpec`; the Rust around them is new.
