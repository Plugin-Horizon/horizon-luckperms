# What Horizon lacks for this mod

Each line is one thing the upstream PR would add. Signatures are written against Horizon `a589a3dd9`
(`crates/horizon-client/src/game/native_hooks.rs`, `native_server.rs`, `lan_actions.rs`).

## Permission layer (the main gap)

Horizon decides who may do what by operator level only (`Guest.permission`, 0 to 4). Paper asks a
permission provider (`Permissible.hasPermission`, which LuckPerms replaces), with the operator status as one
input. The four items below give Horizon the same shape: one provider hook, asked by the engine and by mods,
with the operator level as the fallback for an undefined answer.

- [ ] `fn native_permission_check(&mut self, player: u128, node: &str, default_level: u8, result: &mut Option<bool>)`
  (hook) plus `fn native_has_permission(&self, player: u128, node: &str, default_level: u8) -> bool` (helper:
  asks the hook, falls back to `permission >= default_level` when every mod leaves `result` as `None`) — needed
  for: one permission check every mod and Horizon itself can call without depending on this crate (Bukkit
  `Player.hasPermission`, `/lp verbose`, Horizon's own level checks such as `S::ChangeGameMode ... permission >= 2`
  in `lan_actions.rs`, command blocks, `/tempban`) — today: other mods can call `horizon_luckperms_core::api`
  directly (a crate dependency); Horizon's own checks stay level-only; `/lp verbose` is not possible.
- [ ] `fn native_player_command_level(&mut self, player: u128, name: &str, command: &str, level: &mut u8)` (hook),
  called in `Game::guest_command` (`lan_actions.rs`) after `native_player_command` and before the native-command
  level check and both `host.commands.push(GuestCommand { permission, .. })`; the mod may raise or lower the
  level this one command runs with (it becomes `GuestCommand.permission`, which `run_guest_command` swaps into
  `ui.registries.permission`) — needed for: granting a vanilla or mod command to a non-operator through
  `minecraft.command.<name>` (Paper: `VanillaCommandWrapper` checks that node instead of the level) — today:
  refusing works (`native_player_command` cancel); granting only through the `grant-via-execute` stopgap
  (`execute as <player> at <player> run ...` from the console on the next tick: runs at level 4, its feedback goes
  to the log, not the player). Off by default.
- [ ] `NativeCommand { usage, level, permission: Option<String> }` — the node a mod's command needs; the hub's
  command tree (`lan::commands`, `command_tree`'s levels) and the dispatch in `guest_command` then ask
  `native_has_permission(player, node, level)` instead of comparing `level` — needed for: per-command nodes of
  mod commands (`essentials.tpa`, `voicechat.*`), as Bukkit's `plugin.yml` `permission:` does — today: this mod
  maps command names to nodes in `[horizon.command-nodes]` and can only refuse.
- [ ] `fn native_command_tree_level(&mut self, player: u128, root: &str, level: &mut u8)` (hook) asked by
  `lan::commands` for each root of the tree it sends a player, or a `names_hidden: &mut Vec<String>` argument on
  `native_player_commands_sent` listing what the level hid, so a mod can put a name back — needed for: a granted
  command shows and completes in the player's chat box (LuckPerms `update-client-command-list`) — today:
  `native_player_commands_sent` can only remove names, so refused commands leave the tree, granted ones never
  appear.
- [ ] `fn native_resend_commands(&mut self, player: u128) -> bool` (helper; Bukkit `Player.updateCommands`) —
  needed for: re-sending a player's command tree after `/lp` (or a sync from another server) changed their
  permissions — today: the tree is rebuilt only on join and on an operator level change.

## Logins

- [ ] An asynchronous or deferrable login: `fn native_player_pre_login(&mut self, player: u128, name: &str,
  defer: &mut Option<std::sync::mpsc::Receiver<Result<(), Text>>>)` (the hub holds the login until the receiver
  answers or a timeout passes; `Err` disconnects with the text) — needed for: LuckPerms loads a user's data before
  the player is placed and refuses the login when the database fails (`AsyncPlayerPreLoginEvent`) — today: the
  load starts in `native_player_login` on the worker thread; until it lands (milliseconds) the player resolves as
  a member of `default` only, and a database failure does not refuse the login.

## Output

- [ ] `fn native_console_log(&self, level: &str, line: &str)` (helper around `dedicated::log`) — needed for:
  answers to a console `/lp` that arrive after the hook returned (the worker runs it), log notifications, the
  messenger's "Received update ping" lines, in the server's own log format and to `horizon-server console`
  clients — today: `eprintln!` to stderr.
- [ ] `fn native_level_name(&self) -> Option<String>` (helper: `server.properties` `level-name`) — needed for: the
  `world` context, which Paper builds from world names (`world`, `world_nether`, `world_the_end`) — today: the
  file name of `native_world_dir()`, which is the same for a default server.

## For mods that use the prefix (not LuckPerms itself)

LuckPerms only stores and resolves prefixes; a chat or tab-list mod shows them. Those mods will need:

- [ ] `fn native_chat_decorate(&mut self, player: u128, name: &str, message: &str, decorated: &mut Option<Text>)`
  (Paper `AsyncChatDecorateEvent` / `ChatRenderer`) — needed for: `<prefix> name: message` in chat —
  today: `native_player_chat` cannot change the text (signed chat).
- [ ] `fn native_set_tab_list_name(&mut self, player: u128, name: Option<Text>) -> bool` and
  `fn native_set_display_name(&mut self, player: u128, name: Option<Text>) -> bool` — needed for: prefixes in the
  Tab list and above heads — today: not possible.

## Not needed from Horizon

- Crates: `mysql` 27 (`minimal-rust`, `rustls-tls-ring`), `serde_json`, `toml`, `regex`, `getrandom` are all in
  Horizon's `Cargo.lock` (`horizon-mod-deps`).
- Context changes: the mod reads `native_server_players()` (dimension, game mode, level) every five ticks and right
  before each command decision.

## Command output (round 3)

- [ ] Chat components with hover and click events (`Text::legacy` only takes `§` strings): needed for LuckPerms' "click to
  remove this node" suggestions on `permission info`, `parent info`, `meta info`, `search` and for the clickable
  `/lp help` command list.
- [ ] The label the player typed (`lp`, `luckperms`, `perm`, ...) passed to the command (`CommandManager.execute(label)`):
  usage lines now always say `/lp`.
