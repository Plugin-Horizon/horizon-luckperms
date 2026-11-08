// Completion of /lp's arguments (the replaced range: the word being typed).
fn on_tab_complete(
    player: u128,
    name: &str,
    command: &str,
    suggestions: &mut Vec<horizon_client::mod_api::Suggestion>,
) {
    let Some((start, words)) = horizon_luckperms_core::glue::complete(player, name, command) else {
        return;
    };
    for w in words {
        suggestions.push(horizon_client::mod_api::Suggestion {
            range: (start, command.len()),
            text: w,
            tooltip: None,
            int_value: None,
        });
    }
}
