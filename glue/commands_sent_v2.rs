// The command tree, second pass: roots the player's level hides but a node grants go back in.
fn on_commands_sent_v2(player: u128, names: &mut Vec<String>, names_hidden: &mut Vec<String>) {
    horizon_luckperms_core::glue::unhide_granted(player, names, names_hidden);
}
