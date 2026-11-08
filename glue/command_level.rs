// The level a typed command runs at: a `minecraft.command.<name>` node set true lifts it to
// what the vanilla command needs (replaces the old `grant-via-execute` console stopgap).
fn on_command_level(player: u128, command: &str, level: &mut u8) {
    *level = horizon_luckperms_core::glue::command_level(player, command, *level);
}
