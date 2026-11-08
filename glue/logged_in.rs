// Placed: the players (with this one) to the cache at once, so their contexts are known.
// A fake player (a mod's bot with no client) is not listed: its contexts are not needed, and
// listing every player for each of hundreds of bots was quadratic.
fn on_logged_in(game: &mut crate::game::Game<'_>, player: u128) {
    if game.native_is_player_fake(player) {
        return;
    }
    let players = game
        .native_server_players()
        .into_iter()
        .filter(|p| !p.fake)
        .map(|p| horizon_luckperms_core::runtime::OnlinePlayer {
            uuid: p.uuid,
            name: p.name,
            op_level: p.permission,
            dimension: p.dimension,
            game_mode: p.game_mode,
        })
        .collect();
    horizon_luckperms_core::glue::players(players);
}
