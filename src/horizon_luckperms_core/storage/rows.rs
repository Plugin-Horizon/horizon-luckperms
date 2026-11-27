//! The row form of a node shared by the SQL storages (MariaDB/MySQL and SQLite): the `server` and `world`
//! columns, the `contexts` JSON and the expiry.

use crate::horizon_luckperms_core::context::{ContextSet, SERVER, WORLD};
use crate::horizon_luckperms_core::node::Node;

/// `SqlStorage.readNode`: server and world columns become contexts (`global` is none).
pub(super) fn read_node(id: i64, permission: String, value: bool, server: String, world: String, expiry: i64, contexts: String) -> Option<Node> {
    if permission.is_empty() {
        return None;
    }
    let mut set = ContextSet::new();
    set.add(SERVER, if server.is_empty() { "global" } else { &server });
    set.add(WORLD, if world.is_empty() { "global" } else { &world });
    set.add_all(&ContextSet::from_json(&contexts));
    Some(Node::from_row(permission, value, expiry, set, id))
}

/// `SqlStorage.writeNode`'s columns: (permission, value, server, world, expiry, contexts). The
/// first server and world value (sorted) go into their columns, the rest into the JSON.
pub(super) fn node_columns(n: &Node) -> (String, bool, String, String, i64, String) {
    let mut rest = n.contexts.clone();
    let server = n.contexts.values(SERVER).first().map(|s| s.to_string());
    let world = n.contexts.values(WORLD).first().map(|s| s.to_string());
    rest.remove_key(SERVER);
    rest.remove_key(WORLD);
    for v in n.contexts.values(SERVER).iter().skip(1) {
        rest.add(SERVER, v);
    }
    for v in n.contexts.values(WORLD).iter().skip(1) {
        rest.add(WORLD, v);
    }
    let expiry = if n.has_expiry() { n.expiry } else { 0 };
    (
        n.key.clone(),
        n.value,
        server.unwrap_or_else(|| "global".into()),
        world.unwrap_or_else(|| "global".into()),
        expiry,
        rest.to_json(),
    )
}
