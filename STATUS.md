Plan run: `horizon-mixins plan` passes on Horizon main `b13dbf06` (not built; `cargo` was not run). The MariaDB backend
is back (commit 4dd013c): `core/src/storage/sql.rs` on the `mysql` 28 dependency, which is the 28.0.4 in Horizon's
`Cargo.lock`. The default `storage.method` is `mariadb` (`memory` keeps everything in RAM, lost on restart, for tests and a lone
server). The SQL messenger (`messaging-service = "auto"`) follows the storage method. The file/JSON storage the brief asked to
keep does not exist in this port (see the last table row).

Blind port: written without an engine build or a server. The core crate (`core/`) is checked and unit-tested on
its own (`cargo test`: 58 tests pass with the `mysql` feature, 55 without, against Horizon's `Cargo.lock`); the helpers in `glue/`
and `horizon-mixin.toml` have never been compiled into Horizon.

## Features of LuckPerms (server side) and where they stand

| Feature | State | Notes |
|---|---|---|
| MariaDB/MySQL storage, LuckPerms' own schema and prefix (`user_permissions`, `group_permissions`, `players`, `groups`, `tracks`, `actions`) | written | Back on `mysql` 28 (in Horizon's `Cargo.lock`); not compiled yet | Same statements as `SqlStorage`; server/world columns, `contexts` JSON, `expiry`, deletes by row id |
| Creating the tables when missing, `default` group | written | SQL tables created when missing; `default` group created by both storages | Same `CREATE TABLE`s as `schema/mariadb.sql` |
| Saving only what changed (diff against the last stored state) | written | Users with nothing but `default` lose their rows, as in LuckPerms |
| Players table: name on login, other uuids with the same name dropped, primary group | written | |
| SQL messaging service (`{prefix}messenger`): poll every second, 30 s window, housekeeping every 30 s | written | `update`, `userupdate`, `log`, `custom` in LuckPerms' JSON; ids seen for 5 min |
| Update pushes: user changes ping at once, group/track changes buffered 2 s, `networksync` | written | |
| `sync-minutes`, full sync on an `update` ping, user reload on `userupdate` | written | |
| Contexts: `server`, `world` (Paper's world names per dimension, `world-rewrite`), `dimension-type`, `gamemode`, static contexts, `disabled-contexts` | written | |
| Context matching (at least one value per key), `include-global`, `include-global-world`, `apply-global-groups`, `apply-global-world-groups` | written | |
| Inheritance: depth-first pre/post order, breadth-first, weights, primary group first, `post-traversal-inheritance-sort` | written | |
| Permission check: direct, regex (`R=`), wildcards (`a.*`, `*`), shorthand (`a.(b|c)`, `{1-3}`), lower-cased keys | written | Processor order as on Bukkit |
| Operators: every undefined permission true at `op-level` and above (`enable-ops`) | written | Horizon has levels; see GAPS.md |
| Bukkit child permissions and plugin default permissions (`apply-bukkit-*`) | not started | No `plugin.yml` permissions on Horizon; `default_level` of the proposed hook replaces them |
| Prefix/suffix stacks (`highest`, `lowest`, `_own`, `_inherited`, `_on_track_`, `_not_on_track_`, `_from_group_`, `_not_from_group_`; duplicates; spacers) | written | |
| Meta values (first in inheritance order), `weight`, `primarygroup` meta | written | `meta-value-selection` other than `inheritance`: not started |
| Primary group: `stored`, `parents-by-weight`, `all-parents-by-weight` | written | |
| Temporary nodes, expiry audit every 3 s (saved), `temporary-add-behaviour` and per-command modifier | written | |
| User load on login, unload a minute after leaving | partly | Async; no login refusal on DB failure (GAPS.md) |
| `/lp user|group ... permission info/set/unset/settemp/unsettemp/check/clear` | written | |
| `/lp user|group ... parent info/set/add/remove/addtemp/removetemp/clear/settrack/cleartrack`, `switchprimarygroup` | written | |
| `/lp user|group ... meta info/set/unset/settemp/unsettemp/addprefix/addsuffix/setprefix/setsuffix/removeprefix/removesuffix` and their temp forms, `clear` | written | |
| `/lp user ... info/promote/demote/showtracks/clear/clone` | written | |
| `/lp group ... info/listmembers/setweight/setdisplayname/showtracks/clear/rename/clone` | written | |
| `/lp track ... info/append/insert/remove/clear/rename/clone`, `listtracks/createtrack/deletetrack` | written | |
| `/lp listgroups/creategroup/deletegroup/search/info/sync/networksync/reloadconfig`, `/lp log recent/search/userhistory/grouphistory/trackhistory/notify` | written | `reloadconfig`: the file watcher reloads on save |
| Command permissions `luckperms.*`, `commands-allow-op` | written | |
| Action log rows and log notifications (`log-notify`, `push-log-entries`, `broadcast-received-log-entries`, `/lp log notify off`) | written | |
| Tab completion of `/lp` | written | |
| `/lp editor`, `applyedits`, `trusteditor` | done (no live socket) | bytebin upload/download, diff apply, tests without network |
| `verbose`, `tree`, `bulkupdate`, `import`, `export`, `translations` | not started | verbose needs the provider hook (GAPS.md) |
| Typed commands gated by `minecraft.command.<name>` (Paper's names) or `[horizon.command-nodes]` | partly | Refusing works; granting needs a hook (stopgap `grant-via-execute`, off) |
| Command tree without refused commands | partly | Removing only; granted commands cannot be added (GAPS.md) |
| API for other mods (`api::has_permission`, `prefix`, `suffix`, `meta_value`, `primary_group`, `groups`) | written | |
| Config reload on save (`config/luckperms.toml`) | written | Storage settings need a restart |
| `group-name-rewrite`, `argument-based-command-permissions`, `require-sender-group-membership-to-modify`, Vault options | not started | Off on the servers this was made for, or Bukkit-only |
| File/JSON storage (the brief asked to keep it; none exists in this port), H2/SQLite/MongoDB/YAML storage, Redis/RabbitMQ/NATS messaging | not started | Needs a file-backed `Storage` impl before anything persists; SQL messaging needs a DB |

Counts: see the table; the MariaDB rows are `written` again (not compiled, not run).

## Test first, once a build is possible

1. **Compiles in Horizon**: `horizon-mixins apply` with this bundle at `a589a3dd9`, then a build. Expected trouble
   spots: the binding types (`&crate::game::native_server::NativeServerPlayer`, `horizon_client::mod_api::Suggestion`
   with `range: (usize, usize)`, `horizon_client::text::Text::legacy`) and `mysql`'s features under the workspace's
   feature unification.
2. **Same data as Paper**: a throwaway MariaDB on its own port; a throwaway Paper + LuckPerms on the same tables.
   Groups, prefixes and a user's nodes made on Paper show on Horizon (`/lp user X info`,
   `/lp user X permission check ...`), and the reverse. Compare the rows both write (`server`, `world`, `contexts`,
   `expiry`).
3. **Messaging**: a change on Paper reaches Horizon within ~1 s (`Received update ping` / user reload) and the
   reverse; log notifications cross both ways; nothing is handled twice.
4. **Gate**: `minecraft.command.gamemode false` on an operator refuses `/gamemode`; `lp` itself is never gated.
5. **Login timing**: how long a joining player resolves as `default` only; whether a command typed in the first
   tick sees their real groups.
6. Tab completion ranges of `/lp user <Tab>`.

## Review against the public docs

Source: the public docs repo github.com/LuckPerms/wiki (raw `pages/*.md`): Command-Usage, General-, User-, Group-,
Track-, Permission-, Parent-, Meta-, Log-Commands, Permissions, Configuration. Compared: every sub-command and its
arguments, the permission node list, and the config keys' defaults.

Changed in this pass (not compiled, tests not run):
- `/lp search`: comparators `==` (default), `!=`, `~~` and `!~` as in the docs; `~~`/`!~` are SQL `LIKE` patterns
  (`%`, `_`), no more prefix guessing. SQL and in-memory storage (`storage/mod.rs` `KeyMatch`, `storage/sql.rs`).
- `/lp log search <query> [page]` (`luckperms.log.search`): filters the newest 5000 actions (source, target, text) in
  the mod, not in SQL. Guess: the wiki does not say how far back it searches.
- `/lp promote|demote ... -s`: silent flag, drops the track progress lines (the `-s` argument was read as a context).
- Tab completion of `/lp log` lists `search`.
- `core/src/command/tests.rs`: `search` test uses `%` patterns; `!~` checked.

Matches the docs (no change): every sub-command name and argument order of user, group, track, permission, parent,
meta and log; `--dont-add-to-first`/`--dont-remove-from-first`; config defaults `sync-minutes -1`, `temporary-add-behaviour
deny`, `primary-group-calculation parents-by-weight`, `inheritance-traversal-algorithm depth-first-pre-order`,
`include-global*`, `apply-global-*`, `enable-ops`, `commands-allow-op`, `log-notify`, `auto-push-updates`,
`push-log-entries`, `broadcast-received-log-entries`, `prevent-primary-group-removal false`.

Still open (docs say so, port does not do it):
- Not ported on purpose: `verbose`, `tree`, `bulkupdate`, `import`, `export`, `translations`,
  `applyedits` (see the table above).
- `default-contexts` (contexts.yml), `auto-op`, `meta-value-selection-default` and per-key `meta-value-selection`,
  `argument-based-command-permissions`, `allow-invalid-usernames`, `debug-logins`: absent from the config.
- `storage.method` default: the docs use `h2`, the port uses `mariadb` on purpose.
- `messaging-service` default: the docs use `none` (SQL when the storage is SQL), the port uses `auto`, the same effect.
- `creategroup <name> [weight] [displayname]`: a non-number second argument becomes the display name (a guess; the docs
  do not say).
- Feedback wording and colours: not checked against the docs (no example chat output was read).

## Round 2: command UX and the operator bug (not compiled)

- `/lp` tree: `glue/register.rs` registers one usage per dispatched sub-command path (296 per alias, six aliases, plus a
  free-text `[args:text]` fallback last). Checked offline against Horizon's `usage_nodes` rules (no parser errors, no kind
  clashes). Not registered on purpose: `verbose`, `tree`, `bulkupdate`, `import`, `export`, `translations`
  (the core answers them with "not available"). Core aliases (`perm`, `inheritance`, `chat`, `checkinherits`, ...) are
  accepted but not suggested. Tree size: about 510 nodes per alias.
- Operator "no permissions" on `/lp` (live server): `glue/command_gate.rs` replaced the online cache with the server's
  list on every typed command. While a guest's packets are handled, Horizon has the hub taken (`tick_lan_packets`), so
  `native_server_players` is empty there and the cache was cleared before the worker read it. Now the cache is only
  replaced when the sender is in the list. Test: an op types `/lp` in chat, expect the sub-command list.
- Usage messages: `<x>` placeholders replaced by `<user>`, `<group>` or `<track>` (`core/src/command/holder.rs`,
  `core/src/command/track.rs`).

## Round 3: the command system ported 1:1 from LuckPerms (not compiled)

Source read: LuckPerms `common/commands/**`, `common/command/**` (`CommandManager`, `ArgumentList`, `CommandSpec`,
`CommandPermission`, `TabCompletions`), `Message.java` and `luckperms_en.properties` (MIT, Copyright (c) lucko and
contributors). Not built or run: cargo was not used.

- `core/src/lang.rs` (generated): every English template of `luckperms_en.properties` as a constant, `{0}` filled by `tr`
  with the parent colour restored after each argument (as adventure does), plus the `luckperms.usage.*` texts.
- `core/src/command/specs.rs` (generated from `CommandSpec`): 101 specs with usage line and arguments.
- `core/src/command/tables.rs`: the command tree (24 main commands, 12 children of `group <x>`, 10 of `user <x>`, 7
  `permission`, 10 `parent`, 18 `meta`, 7 `track`, 6 `log` sub commands) with argument-count predicates and permission nodes.
- `/lp` with no arguments, an unknown or unauthorised main command, a missing sub command, a missing argument, an unknown
  sub command, a refused one: each answers as `CommandManager`/`ParentCommand`/`GenericParentCommand` do (the banner and
  command list; `<Name> Sub Commands: (...)`; `Command Usage - <name>` with `Arguments:`; `Command not recognised.`;
  `You do not have permission to use this command!`). `/lp help` is not a command (it lists the commands).
- Convenience aliases (`u`, `g`, `t`, `i`; `p`, `perm`, `g`, `group`, `m`, `i`, `e`), `{SPACE}`, the original tokenizer
  (quotes only at the start of an argument, double space = empty argument).
- Contexts: a bare word is the server, the second the world (`ArgumentList.parseContext`); durations as
  `DurationParser` (units in order, a plain number is a unix timestamp, past dates refused with their own message);
  temporary modifiers; booleans inserted when omitted.
- Output of every command in LuckPerms' colours (`[LP]` prefix with the bold letters, `&b` names, `&a` bodies, `&e`global,
  `server=`/`world=` pairs), page sizes (19 permission/parent entries, 15 search, 8 `listgroups`, 10 log), sort modes,
  `permission check` (own, inherited, wildcard lines, then result/processor/cause/context with `common.DirectProcessor` ...),
  `info` pages of user and group, `meta info`, `showtracks`, promote/demote with the track path, `clear` counts.
- Config: `allow-invalid-usernames`, `disable-luckperms-commands`, `commands-read-only-mode`,
  `log-notify-filtered-descriptions` added; defaults checked against `config.yml`.
- Tab completion follows `tabComplete` of each command (sub command names the sender may use, groups, tracks, booleans,
  contexts); the registered usage tree (`glue/register.rs`) adds `[page:integer]`, the `--update-parent-lists` literal and
  the commands that answer "not available".

Differences that remain (and why):

- No hover or click events (the glue turns `§` strings into plain components): "click to remove" suggestions are missing.
- `verbose`, `tree`, `bulkupdate`, `import`, `export`, `translations` are listed and
  answer "not available on Horizon" (they need files or the verbose hook).
- `argument-based-command-permissions` and `require-sender-group-membership-to-modify` are not ported (off by default).
- Shorthand parse warnings on `permission set` and MiniMessage values are not ported; `&` and `§` values are coloured.
- `/lp` always prints `/lp` in usage lines (the label typed is not passed on); the version printed is the one in
  `command::LUCKPERMS_VERSION`.
- `permission info`/`meta info` use this port's node order (`NodeSet::sorted`), `search` sorts holders by name.
- `reloadconfig` only reports: the file watcher reloads `config/luckperms.toml` within a second of a save.
- Storage details `info` prints for SQL (ping, connected) are left out.
