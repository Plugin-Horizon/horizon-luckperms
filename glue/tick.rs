// Twice a tick (start and end phase): every 10th call the players go to the cache and the config
// file is looked at; every call the answers and notifications go out.
fn on_tick(game: &mut crate::game::Game<'_>) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static CALLS: AtomicU32 = AtomicU32::new(0);
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    let players = (n % 10 == 0).then(|| {
        game.native_server_players()
            .into_iter()
            // A fake player (a mod's bot with no client) is not listed.
            .filter(|p| !p.fake)
            .map(|p| horizon_luckperms_core::runtime::OnlinePlayer {
                uuid: p.uuid,
                name: p.name,
                op_level: p.permission,
                dimension: p.dimension,
                game_mode: p.game_mode,
            })
            .collect()
    });
    if n % 40 == 0 {
        horizon_luckperms_core::glue::check_config();
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
    let out = horizon_luckperms_core::glue::tick(players);
    for o in out.lines {
        match o.to {
            horizon_luckperms_core::Recipient::Player(uuid) => {
                game.native_tell(uuid, line_text(&o.line));
            }
            horizon_luckperms_core::Recipient::Console => {
                for l in horizon_luckperms_core::text::plain(&o.line).lines() {
                    eprintln!("[LuckPerms] {l}");
                }
            }
        }
    }
    // A group, user or track changed: everyone's command tree again, so grants show at once.
    if out.resend_trees {
        for p in game.native_server_players().into_iter().filter(|p| !p.fake) {
            game.native_resend_commands(p.uuid);
        }
    }
    // Queued console commands (no player is swapped in during the tick hook).
    for line in out.console_commands {
        if !game.native_run_command(&line) {
            eprintln!("[LuckPerms] could not run: {line}");
        }
    }
}
