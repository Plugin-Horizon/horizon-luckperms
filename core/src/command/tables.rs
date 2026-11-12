//! The command tree: which sub commands each command has, with the spec (usage text), the argument-count check and the
//! permission node of each. Mirrors the constructors of the original's command classes.

use super::args::Check;

/// A sub command of a shared parent (`permission`, `parent`, `meta`) or a plain child.
#[derive(Clone, Copy)]
pub struct Sub {
    pub name: &'static str,
    pub spec: &'static str,
    pub check: Check,
    /// `permission.set`: the part after `luckperms.user.` / `luckperms.group.`.
    pub node: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Only {
    Both,
    User,
    Group,
}

/// A child of `user <x>` / `group <x>`: a command, or a parent with its own sub commands.
#[derive(Clone, Copy)]
pub struct Child {
    pub name: &'static str,
    /// The name its usage header shows (`Permission` for the parent).
    pub shown: &'static str,
    pub spec: &'static str,
    pub check: Check,
    pub node: &'static str,
    pub subs: &'static [Sub],
    pub only: Only,
}

const fn sub(name: &'static str, spec: &'static str, check: Check, node: &'static str) -> Sub {
    Sub { name, spec, check, node }
}

pub const PERMISSION_SUBS: &[Sub] = &[
    sub("info", "permission-info", Check::NotInRange(0, 2), "permission.info"),
    sub("set", "permission-set", Check::Is(0), "permission.set"),
    sub("unset", "permission-unset", Check::Is(0), "permission.unset"),
    sub("settemp", "permission-settemp", Check::InRange(0, 2), "permission.settemp"),
    sub("unsettemp", "permission-unsettemp", Check::Is(0), "permission.unsettemp"),
    sub("check", "permission-check", Check::Is(0), "permission.check"),
    sub("clear", "permission-clear", Check::AlwaysFalse, "permission.clear"),
];

pub const PARENT_SUBS: &[Sub] = &[
    sub("info", "parent-info", Check::NotInRange(0, 2), "parent.info"),
    sub("set", "parent-set", Check::Is(0), "parent.set"),
    sub("add", "parent-add", Check::Is(0), "parent.add"),
    sub("remove", "parent-remove", Check::Is(0), "parent.remove"),
    sub("settrack", "parent-set-track", Check::InRange(0, 1), "parent.settrack"),
    sub("addtemp", "parent-add-temp", Check::InRange(0, 1), "parent.addtemp"),
    sub("removetemp", "parent-remove-temp", Check::Is(0), "parent.removetemp"),
    sub("clear", "parent-clear", Check::AlwaysFalse, "parent.clear"),
    sub("cleartrack", "parent-clear-track", Check::Is(0), "parent.cleartrack"),
    sub("switchprimarygroup", "user-switchprimarygroup", Check::Not(1), "parent.switchprimarygroup"),
];

pub const META_SUBS: &[Sub] = &[
    sub("info", "meta-info", Check::AlwaysFalse, "meta.info"),
    sub("set", "meta-set", Check::InRange(0, 1), "meta.set"),
    sub("unset", "meta-unset", Check::Is(0), "meta.unset"),
    sub("settemp", "meta-settemp", Check::InRange(0, 2), "meta.settemp"),
    sub("unsettemp", "meta-unsettemp", Check::Is(0), "meta.unsettemp"),
    sub("addprefix", "meta-addprefix", Check::InRange(0, 1), "meta.addprefix"),
    sub("addsuffix", "meta-addsuffix", Check::InRange(0, 1), "meta.addsuffix"),
    sub("setprefix", "meta-setprefix", Check::Is(0), "meta.setprefix"),
    sub("setsuffix", "meta-setsuffix", Check::Is(0), "meta.setsuffix"),
    sub("removeprefix", "meta-removeprefix", Check::Is(0), "meta.removeprefix"),
    sub("removesuffix", "meta-removesuffix", Check::Is(0), "meta.removesuffix"),
    sub("addtempprefix", "meta-addtemp-prefix", Check::InRange(0, 2), "meta.addtempprefix"),
    sub("addtempsuffix", "meta-addtemp-suffix", Check::InRange(0, 2), "meta.addtempsuffix"),
    sub("settempprefix", "meta-settemp-prefix", Check::InRange(0, 1), "meta.settempprefix"),
    sub("settempsuffix", "meta-settemp-suffix", Check::InRange(0, 1), "meta.settempsuffix"),
    sub("removetempprefix", "meta-removetemp-prefix", Check::Is(0), "meta.removetempprefix"),
    sub("removetempsuffix", "meta-removetemp-suffix", Check::Is(0), "meta.removetempsuffix"),
    sub("clear", "meta-clear", Check::AlwaysFalse, "meta.clear"),
];

const fn child(name: &'static str, spec: &'static str, check: Check, node: &'static str, only: Only) -> Child {
    Child { name, shown: name, spec, check, node, subs: &[], only }
}

const fn parent(name: &'static str, shown: &'static str, spec: &'static str, subs: &'static [Sub]) -> Child {
    Child { name, shown, spec, check: Check::AlwaysFalse, node: "", subs, only: Only::Both }
}

/// The children of `user <x>` (`tag` = "user") or `group <x>` (`tag` = "group"), in the original's order.
pub fn holder_children(user: bool) -> Vec<Child> {
    let mut v = vec![
        child("info", if user { "user-info" } else { "group-info" }, Check::AlwaysFalse, "info", Only::Both),
        parent("permission", "Permission", "permission", PERMISSION_SUBS),
        parent("parent", "Parent", "parent", PARENT_SUBS),
        parent("meta", "Meta", "meta", META_SUBS),
        child("editor", "holder-editor", Check::AlwaysFalse, "editor", Only::Both),
    ];
    if user {
        v.push(child("promote", "user-promote", Check::AlwaysFalse, "promote", Only::User));
        v.push(child("demote", "user-demote", Check::AlwaysFalse, "demote", Only::User));
    } else {
        v.push(child("listmembers", "group-listmembers", Check::NotInRange(0, 2), "listmembers", Only::Group));
        v.push(child("setweight", "group-setweight", Check::Not(1), "setweight", Only::Group));
        v.push(child("setdisplayname", "group-set-display-name", Check::Is(0), "setdisplayname", Only::Group));
    }
    v.push(child("showtracks", "holder-showtracks", Check::AlwaysFalse, "showtracks", Only::Both));
    v.push(child("clear", "holder-clear", Check::AlwaysFalse, "clear", Only::Both));
    if user {
        v.push(child("clone", "user-clone", Check::Not(1), "clone", Only::User));
    } else {
        v.push(child("rename", "group-rename", Check::NotInRange(1, 2), "rename", Only::Group));
        v.push(child("clone", "group-clone", Check::Not(1), "clone", Only::Group));
    }
    v
}

/// The sub commands of a shared parent that apply to this holder type (`switchprimarygroup` is the user's only).
pub fn parent_subs(ch: &Child, user: bool) -> Vec<Sub> {
    ch.subs.iter().copied().filter(|s| user || s.name != "switchprimarygroup").collect()
}

pub const TRACK_CHILDREN: &[Child] = &[
    child("info", "track-info", Check::AlwaysFalse, "info", Only::Both),
    child("editor", "track-editor", Check::AlwaysFalse, "editor", Only::Both),
    child("append", "track-append", Check::Not(1), "append", Only::Both),
    child("insert", "track-insert", Check::Not(2), "insert", Only::Both),
    child("remove", "track-remove", Check::Not(1), "remove", Only::Both),
    child("clear", "track-clear", Check::AlwaysFalse, "clear", Only::Both),
    child("rename", "track-rename", Check::Not(1), "rename", Only::Both),
    child("clone", "track-clone", Check::Not(1), "clone", Only::Both),
];

pub const LOG_CHILDREN: &[Child] = &[
    child("recent", "log-recent", Check::NotInRange(0, 2), "recent", Only::Both),
    child("search", "log-search", Check::Is(0), "search", Only::Both),
    child("notify", "log-notify", Check::NotInRange(0, 1), "notify", Only::Both),
    child("userhistory", "log-user-history", Check::NotInRange(1, 2), "userhistory", Only::Both),
    child("grouphistory", "log-group-history", Check::NotInRange(1, 2), "grouphistory", Only::Both),
    child("trackhistory", "log-track-history", Check::NotInRange(1, 2), "trackhistory", Only::Both),
];

/// What authorises a main command.
#[derive(Clone, Copy)]
pub enum Auth {
    /// One node.
    Node(&'static str),
    /// Any child of `user <x>`.
    User,
    /// Any child of `group <x>`.
    Group,
    /// Any child of `track <x>`.
    Track,
    /// Any child of `log`.
    Log,
}

#[derive(Clone, Copy)]
pub struct Main {
    /// Lower-case command word.
    pub name: &'static str,
    pub spec: &'static str,
    pub check: Check,
    pub auth: Auth,
    /// `shouldDisplay`
    pub shown: bool,
    /// The plugin does not do it here (web editor, imports, ...): the command answers so.
    pub unsupported: bool,
}

const fn main(name: &'static str, spec: &'static str, check: Check, auth: Auth) -> Main {
    Main { name, spec, check, auth, shown: true, unsupported: false }
}

const fn hidden(name: &'static str, spec: &'static str, check: Check, node: &'static str) -> Main {
    Main { name, spec, check, auth: Auth::Node(node), shown: false, unsupported: false }
}

const fn unsupported(name: &'static str, spec: &'static str, node: &'static str, shown: bool) -> Main {
    Main { name, spec, check: Check::AlwaysFalse, auth: Auth::Node(node), shown, unsupported: true }
}

/// `CommandManager.mainCommands`, in order.
pub const MAINS: &[Main] = &[
    main("user", "user", Check::AlwaysFalse, Auth::User),
    main("group", "group", Check::AlwaysFalse, Auth::Group),
    main("track", "track", Check::AlwaysFalse, Auth::Track),
    main("log", "log", Check::AlwaysFalse, Auth::Log),
    main("sync", "sync", Check::AlwaysFalse, Auth::Node("sync")),
    main("info", "info", Check::AlwaysFalse, Auth::Node("info")),
    main("editor", "editor", Check::NotInRange(0, 2), Auth::Node("editor")),
    unsupported("verbose", "verbose", "verbose", true),
    unsupported("tree", "tree", "tree", true),
    main("search", "search", Check::NotInRange(1, 3), Auth::Node("search")),
    main("networksync", "network-sync", Check::AlwaysFalse, Auth::Node("sync")),
    unsupported("import", "import", "import", true),
    unsupported("export", "export", "export", true),
    main("reloadconfig", "reload-config", Check::AlwaysFalse, Auth::Node("reloadconfig")),
    unsupported("bulkupdate", "bulk-update", "bulkupdate", true),
    unsupported("translations", "translations", "translations", true),
    hidden("applyedits", "apply-edits", Check::NotInRange(1, 2), "applyedits"),
    hidden("trusteditor", "trust-editor", Check::Not(1), "trusteditor"),
    main("creategroup", "create-group", Check::NotInRange(1, 3), Auth::Node("creategroup")),
    main("deletegroup", "delete-group", Check::NotInRange(1, 2), Auth::Node("deletegroup")),
    main("listgroups", "list-groups", Check::NotInRange(0, 1), Auth::Node("listgroups")),
    main("createtrack", "create-track", Check::Not(1), Auth::Node("createtrack")),
    main("deletetrack", "delete-track", Check::Not(1), Auth::Node("deletetrack")),
    main("listtracks", "list-tracks", Check::AlwaysFalse, Auth::Node("listtracks")),
];

pub fn find_main(name: &str) -> Option<&'static Main> {
    MAINS.iter().find(|m| m.name == name)
}

/// `CommandPermission.isReadOnly`
pub fn read_only(node: &str) -> bool {
    const PLAIN: [&str; 11] = ["sync", "info", "editor", "verbose", "tree", "search", "export", "reloadconfig", "translations", "listgroups", "listtracks"];
    if PLAIN.contains(&node) {
        return true;
    }
    if node.starts_with("log.") {
        return true;
    }
    let Some((tag, rest)) = node.split_once('.') else { return false };
    match tag {
        "user" | "group" => {
            matches!(rest, "info" | "permission.info" | "permission.check" | "parent.info" | "meta.info" | "editor" | "showtracks")
                || (tag == "group" && rest == "listmembers")
        }
        "track" => matches!(rest, "info" | "editor"),
        _ => false,
    }
}
