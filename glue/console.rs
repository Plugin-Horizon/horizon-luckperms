// /lp on the server console.
fn on_console(command: &str, ran: &mut bool) {
    if horizon_luckperms_core::glue::lp_command(None, command) {
        *ran = true;
    }
}
