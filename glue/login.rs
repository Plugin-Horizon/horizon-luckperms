// A player was accepted: their name to the players table, their data loaded (on the worker).
// A fake player (a mod's bot with no client) is not loaded: it resolves as a member of `default`
// in memory, with no database work.
fn on_login(game: &mut crate::game::Game<'_>, player: u128, name: &str) {
    if game.native_is_player_fake(player) {
        return;
    }
    horizon_luckperms_core::glue::login(player, name);
}
