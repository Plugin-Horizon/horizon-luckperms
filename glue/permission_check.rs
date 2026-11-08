// Every node any mod or the server asks about (`native_has_permission`): LuckPerms answers
// when the node is set for the player in their contexts; undefined leaves it to the operator
// level. Asked through `&self` with the hub out of reach: only the core's own cache is read.
fn on_permission_check(player: u128, node: &str, result: &mut Option<bool>) {
    if result.is_none() {
        *result = horizon_luckperms_core::glue::permission(player, node);
    }
}
