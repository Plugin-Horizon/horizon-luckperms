// A player's /lp: handed to the worker; the answer comes with a later tick.
fn on_perform(player: &crate::game::native_server::NativeServerPlayer, command: &str, ran: &mut bool) {
    if horizon_luckperms_core::glue::lp_command(Some((player.uuid, &player.name)), command) {
        *ran = true;
    }
}
