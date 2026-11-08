// The command tree a player gets: refused commands are left out.
fn on_commands_sent(player: u128, names: &mut Vec<String>) {
    horizon_luckperms_core::glue::filter_tree(player, names);
}
