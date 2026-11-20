Parity: before ~70% -> after ~73% (~343/~470 items; method: audit + PARITY.md estimate; items = commands and sub commands, nodes, config keys, messages, features; an item behind an off-by-default feature or a missing engine API counts as not done)

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
