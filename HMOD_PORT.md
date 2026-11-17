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
