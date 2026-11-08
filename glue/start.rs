// Server start: config/luckperms.toml, the database, every group and track.
fn on_start(game: &mut crate::game::Game<'_>) {
    let config_dir = match crate::game::Game::native_config_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("[LuckPerms] no config folder ({e}); not started");
            return;
        }
    };
    // The world folder's name is the level name Paper's world contexts are built from.
    let level = game
        .native_world_dir()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "world".to_owned());
    horizon_luckperms_core::glue::start(&config_dir, &level);
}
