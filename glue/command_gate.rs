// Every command a player types, before anything looks at it: refused when its node is false.
// The player's state is not swapped in here, so the answer goes through `replies`.
fn on_command_gate(
    game: &mut crate::game::Game<'_>,
    player: u128,
    name: &str,
    command: &str,
    canceled: &mut bool,
    replies: &mut Vec<horizon_client::text::Text>,
) {
    // Fresh contexts for the decision (dimension and game mode may have changed since the last tick).
    // Taken while the guest's packets are handled, the server's players are out of reach here
    // (`native_server_players` is empty), so the list may leave the sender out: then the cache
    // keeps them, or the worker's /lp would find nobody online and say "no permissions".
    let players: Vec<horizon_luckperms_core::runtime::OnlinePlayer> = game
        .native_server_players()
        .into_iter()
        .map(|p| horizon_luckperms_core::runtime::OnlinePlayer {
            uuid: p.uuid,
            name: p.name,
            op_level: p.permission,
            dimension: p.dimension,
            game_mode: p.game_mode,
        })
        .collect();
    let level = players.iter().find(|p| p.uuid == player).map(|p| p.op_level);
    if level.is_some() {
        horizon_luckperms_core::glue::players(players);
    }
    // A chat line as Horizon text: legacy codes, with the click and hover of its rich segments.
    let line_text = |line: &str| -> horizon_client::text::Text {
        use horizon_client::text::{ClickEvent, Style, Text};
        use horizon_luckperms_core::text::Click;
        let mut out = Text::empty();
        for seg in horizon_luckperms_core::text::segments(line) {
            let text = Text::legacy(&seg.text);
            if seg.click.is_none() && seg.hover.is_none() {
                out.push(text);
                continue;
            }
            let style = Style {
                click: seg.click.map(|c| match c {
                    Click::OpenUrl(v) => ClickEvent::OpenUrl(v),
                    Click::SuggestCommand(v) => ClickEvent::SuggestCommand(v),
                    Click::RunCommand(v) => ClickEvent::RunCommand(v),
                }),
                hover: seg.hover.map(|h| Box::new(Text::legacy(&h))),
                ..Style::default()
            };
            out.push(text.with_style(&style));
        }
        out
    };
    match horizon_luckperms_core::glue::gate(player, name, command, level.unwrap_or(0)) {
        horizon_luckperms_core::glue::Gate::Pass => {}
        horizon_luckperms_core::glue::Gate::Refuse(line) => {
            *canceled = true;
            replies.push(line_text(&line));
        }
    }
}
