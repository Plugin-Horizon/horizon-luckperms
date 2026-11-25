Parity: 98% now (286/291 items of the audit list; the 5 open ones are wave 2: written behind the cargo feature `wave2`, they need fam-mod-deps-db-clients and fam-provider-bridges; n/a lines are not counted). Older estimate: 76% now / 77% with our PRs / 77% with engine changes / 77% with new SDK (~358/~470 items; estimate from the round 3 count plus this round; items = commands and sub commands, nodes, config keys, messages, features; an item behind an off-by-default feature or a missing engine API counts as not done)

## Round 100 (blind, not compiled): checklist and closed items
Closed this round: `/lp export` (file, `--without-users`, `--without-groups`, `--upload`), `/lp import` (file, `--replace`,
`--upload`, the web editor's reply), the export/import messages, `unique_users` in storage (players table plus node rows),
config keys `allow-invalid-usernames` (was read nowhere), `context-satisfy-mode`, `disabled-context-calculators`, `debug-logins`.
Messenger-only credential keys (`redis.*`, `nats.*`, `rabbitmq.*` user, password, token) listed in `IGNORED` with the reason.

Checklist round 100 (ticked = matches the original; the Paper side is `common/.../commands` and `luckperms_en.properties`):
- [x] Commands, 24 Paper commands: user, group, track, log, sync, info, editor, search, networksync, reloadconfig, bulkupdate, tree,
      creategroup, deletegroup, listgroups, createtrack, deletetrack, listtracks, export, import (`specs.rs` has the 24 Paper usage
      lines of `lp_specs.txt`; its Sponge spec is there too, unused).
- [~] verbose: on/record/off/upload/paste and filters yes; `verbose command` answers "not available" (no engine call, GAPS_UPSTREAM).
- [~] trusteditor and applyedits: the reply and the apply work; the live editor socket (bytesocks) is not ported.
- [ ] translations: listing and `install` not ported (English only; the install messages exist in `lang.rs` but nothing uses them).
- [x] Sponge `sponge*` commands and `option.*` nodes: Sponge only, not applicable (not counted).
- [x] Permission nodes: 126 Paper nodes; 125 are checked by the port. [ ] `verbose.command.others` (with `verbose command`).
- [x] Config keys: each of the 101 keys of Bukkit `config.yml` is read or listed in `IGNORED` (script check; the
      `vault-*`, `redis`/`nats`/`rabbitmq`/`mongodb`/`split-storage` keys are refused or ignored). About 60 are acted on; 31 are read and ignored (`IGNORED`); about 10
      are noted at start and not applied (`unsupported`, see Known gaps).
- [x] Messages: all command, usage (279/279), log, duration, login and command-system texts are in `lang.rs` (the `''` of the
      properties file is one `'` there; 343/343 command texts, 26 of them differ only by that).
- [x] Behaviour: login load (refusal with reason, grace window), logout and the housekeeper unload (`debug-logins`), permission
      handler (Boolean answers, operator fallback, offline UUIDs refused), command gate, tab completion, contexts (server, world,
      dimension-type, gamemode, static; calculators by name), SQL messaging, verbose recording at the handler, bulkupdate and
      export/import on the worker thread.
- [ ] Storage types other than MariaDB/MySQL and `memory` (h2, sqlite, postgresql, mongodb, yaml, json, hocon, yaml-combined,
      split storage): refused at start. Library work, no engine API.
- [ ] Messaging redis, nats, rabbitmq, pluginmsg: refused. Library work, no engine API.
- [ ] `require-sender-group-membership-to-modify`: read, not enforced (about a dozen group-changing commands need the check).
- [ ] `argument-based-command-permissions`, `meta-value-selection`, `apply-default-negated-permissions-before-wildcards`,
      `resolve-command-selectors`, `auto-op`: read, refused or noted, not applied.
- [ ] Bukkit child and default permissions (`plugin.yml` children and `default`): no node children on Horizon.
- [ ] Granted vanilla commands for non-operators (`minecraft.command.<name>` true): API-blocked (GAPS_UPSTREAM, new line).
- [ ] Handler default and `Nobody`: behind `horizon_handler_default` (off) and the PRs horizon-rust/horizon#18755 and #18754.
- [x] Vault (services `vault:permission` and `vault:chat`), the PlaceholderAPI expansion (`luckperms:placeholder`): ported in round 6.

API-blocked items with a branch or PR: #18754 (`Nobody`) and #18755 (declared default to the handler) (+2 with our PRs).
API-blocked items without one: verbose command (`run_command_as_player_sync`), granted vanilla commands (new GAPS line).
Not compiled: this round is blind; the next compile round has to run `cargo test` in `core/` and the unit tests in
`command/backup.rs` (time and JSON), `original.rs` (the new keys) and `command/tests.rs`.

## Round 3 (blind, not compiled)
- Added `/lp verbose` (on, record, off, upload; filter expressions; rate limit; bytebin upload) in `verbose.rs` + `command/extra.rs`,
  fed by the handler (`glue::handler_answer`). `/lp tree` (known nodes, bytebin upload, reference user), `/lp bulkupdate` (data types,
  delete/update of permission/server/world, constraints `== != ~~ !~`, `-s`, confirm id with 30 s expiry, console only).
- Config: `disable-bulkupdate`, `skip-bulkupdate-confirmation`, `verbose-viewer-url`, `tree-viewer-url` (read from `config.yml` too),
  `horizon.login-wait-ms`.
- Login: `player_login` waits (bounded) for the data and refuses the connection with a reason; 0 keeps the grace window.
- Handler default: cargo feature `horizon_handler_default` (off) reads `event.default` (horizon-rust/horizon#18755) and falls back to the
  node's declared default (`glue::declared_answer`, unit tested).
- Open: storage backends h2/sqlite/yaml/json/hocon/mongodb/postgres (the SQL layer is written against the `mysql` crate; the file
  formats need a Configurate-compatible reader/writer), redis/nats/rabbitmq messaging, import/export, translations, `verbose command`,
  Vault, the PlaceholderAPI expansion, the LuckPerms API, granted vanilla commands (engine API, see GAPS_UPSTREAM), Bukkit child permissions.

# hmod port: luckperms
Round 1
- Fixed:
  - `src/lib.rs` imports: removed `PlayerPermissionCheckEvent` (absent on main) and `Host` (only used by the removed hook).
  - `src/lib.rs` `settle_logins`: `kick` replaced by `disconnect` (`PlayerRef::disconnect(host, Text)`, the `player_disconnect` host method).
  - `src/lib.rs`: `player_permission_check` hook removed (no such hook on main). Replaced by a `HMOD-GAP(handler)` comment in its place.
- Permission nodes: none declared. LuckPerms is a provider, not a consumer, in this round.
- Gaps:
  - HMOD-GAP(handler): LuckPerms no longer answers node checks. It was the provider through `player_permission_check`. Horizon main has no such hook. The handler port needs:
    1. `permission_gather_handler` pushing a handler id (e.g. `luckperms:permissions`).
    2. `permission_handler_query(host, event)`: when `event.handler` is that id and `event.node.value_type` is Boolean, answer `Some(Ok(PermissionValue::Boolean(b)))` from `glue::permission(uuid, &event.node.name)`. The `None` case (no LuckPerms opinion) must become an explicit `PermissionError` or a computed default. An unanswered query is a handler failure, never a grant, so the default must be computed in the mod (operator level and the operator list), since the built-in default is not reachable from the handler. Non-Boolean nodes get an error.
    3. Activation: the server's `permission-handler` setting must name that id. Until it does, the built-in handler answers from node defaults and LuckPerms answers nothing.
    4. Loss meanwhile: the `has_permission` answers of every other mod now come from node defaults. Any LuckPerms-granted node is not seen. `glue::permission` is unused in this crate (no compile error).
- Unsure:
  - The `kick` to `disconnect` rename is a direct API rename. `disconnect` returns `Result<bool, TextFormError>`; the result is ignored as before.
  - Warnings about `cfg` value `net` in `bytebin.rs` and `command/editor.rs` remain (warnings only, not touched).

Round 2 (parity fixes)
- Fixed: the HMOD-GAP(handler) is closed. `permission_gather_handler` pushes `luckperms:permissions`;
  `permission_handler_query` answers Boolean nodes from `glue::handler_answer` (LuckPerms decision, else the operator
  check for online players; offline UUID -> `UnknownPlayer`; non-Boolean -> `WrongType`). Activate it with
  `permission-handler=luckperms:permissions` in server.properties.
- Fixed: the default config used `method = "memory"` (data lost at restart); it is `mariadb` again, memory logs a warning.
- Fixed: `/lp` from command blocks, functions and `execute as` is handled (`run_mod_command`).
- Gaps still open: granted vanilla commands, other storage backends, verbose/tree/bulkupdate, login refusal (5 s grace).
- Unsure: a node declared by another mod for everyone (level 0) is denied to non-ops without an explicit
  LuckPerms grant, because the handler cannot read the node's declared default.
Round 3
- granted vanilla commands: `player_commands_sent_restore` puts the hidden, granted roots back into the command tree (`glue::unhide_granted`). Item list 258/263; open: Vault provider (E), trusteditor, storage-method, split-storage, messaging-service.

## Round 4 (2026-11-25, blind, not compiled)
Closed the four items that needed library work, no engine API:
- `/lp trusteditor` and the live editor socket: `editorsocket.rs` (protocol, ECDSA P-256 keys with ring, keystore, bytesocks over ureq + tungstenite),
  `netio.rs` (TCP/TLS/websocket upgrade), `command/editorlive.rs` (the server side: sessions, trust, applying, follow-up session).
- `storage-method`: `sqlite` (`storage/sqlite.rs`, rusqlite), `yaml`/`json`/`hocon`/`toml` and `-combined` (`storage/file.rs`, `storage/formats.rs`).
  n/a with a reason: `h2` (Java database engine), `postgresql`, `mongodb` (no client crate in `horizon-mod-deps`).
- `split-storage` (`storage/split.rs`, `storage/factory.rs`).
- `messaging-service`: `redis` (the redis crate, as husksync) and `nats` (hand-written protocol) in `messenger.rs`; `none` now reads as `auto`
  as in `MessagingFactory`, `notsql` turns messaging off. n/a with a reason: `rabbitmq` (no AMQP client), `postgresql`, `pluginmsg`.
- Config keys now read: `split-storage.*`, `redis.*` (incl. `addresses`, `ssl`, `sentinel.*`), `nats.*` (incl. `ssl`), `rabbitmq.enabled`, `bytesocks-url`.
- Needs a compile round: `cargo test -p mod-luckperms` (new features `redis`, `sqlite`, `wire`; all in the default set).
