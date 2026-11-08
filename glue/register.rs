// /lp and its aliases, for everyone (level 0; /lp checks luckperms.* itself). One usage per sub-command
// path with typed arguments, so the client completes every word and argument as it does for the real
// plugin: `<name:kind>` required, `[name:kind]` optional (last), literal words for fixed choices (true,
// false, the temporary modifiers). The last usage per alias is a free-text fallback, so anything typed
// still parses. Mirrors the sub-commands `core/src/command/mod.rs` dispatches (no verbose/tree/...).
fn on_register(commands: &mut Vec<crate::game::native_server::NativeCommand>) {
    // `<prefix> <duration:word>`, then LuckPerms' optional modifier and context.
    let temp = |prefix: &str, out: &mut Vec<String>| {
        out.push(format!("{prefix} <duration:word>"));
        for m in ["accumulate", "replace", "deny"] {
            out.push(format!("{prefix} <duration:word> {m}"));
            out.push(format!("{prefix} <duration:word> {m} [context:text]"));
        }
        out.push(format!("{prefix} <duration:word> [context:text]"));
    };
    let mut usages: Vec<String> = [
        "info",
        "sync",
        "networksync",
        "reloadconfig",
        "listgroups",
        "listgroups [page:integer]",
        "creategroup <name:word> [weight:word] [displayname:text]",
        "deletegroup <name:word>",
        "deletegroup <name:word> --update-parent-lists",
        "listtracks",
        "createtrack <name:word>",
        "deletetrack <name:word>",
        "search <permission:word> [page:integer]",
        "search <comparison:word> <permission:word> [page:integer]",
        "editor [type:word] [filter:text]",
        "verbose <action:word> [filter:text]",
        "tree [scope:word] [player:word]",
        "import <file:word> [flags:text]",
        "export <file:word> [flags:text]",
        "bulkupdate [args:text]",
        "translations [install:word]",
        "log recent",
        "log recent [page:integer]",
        "log recent <user:player_name> [page:integer]",
        "log userhistory <user:player_name> [page:integer]",
        "log grouphistory <group:word> [page:integer]",
        "log trackhistory <track:word> [page:integer]",
        "log notify",
        "log notify on",
        "log notify off",
        "log search <query:word> [page:integer]",
        "track <track:word>",
        "track <track:word> info",
        "track <track:word> editor",
        "track <track:word> append <group:word>",
        "track <track:word> insert <group:word> <position:integer>",
        "track <track:word> remove <group:word>",
        "track <track:word> clear",
        "track <track:word> rename <name:word>",
        "track <track:word> clone <name:word>",
    ]
    .map(String::from)
    .into_iter()
    .collect();

    // What `/lp user <user>` and `/lp group <group>` both take, below the holder's name.
    let mut shared: Vec<String> = vec![
        "info".into(),
        "showtracks".into(),
        "clear [context:text]".into(),
        "permission info".into(),
        "permission info [page:integer]".into(),
        "editor".into(),
        "permission set <node:word> [context:text]".into(),
        "permission set <node:word> true [context:text]".into(),
        "permission set <node:word> false [context:text]".into(),
        "permission unset <node:word> [context:text]".into(),
        "permission check <node:word> [context:text]".into(),
        "permission unsettemp <node:word> [duration:word] [context:text]".into(),
        "permission clear [context:text]".into(),
        "parent info".into(),
        "parent info [page:integer]".into(),
        "parent set <group:word> [context:text]".into(),
        "parent add <group:word> [context:text]".into(),
        "parent remove <group:word> [context:text]".into(),
        "parent removetemp <group:word> [duration:word] [context:text]".into(),
        "parent clear [context:text]".into(),
        "parent settrack <track:word> <group:word> [context:text]".into(),
        "parent cleartrack <track:word> [context:text]".into(),
        "meta info".into(),
        "meta set <key:word> <value:word> [context:text]".into(),
        "meta unset <key:word> [context:text]".into(),
        "meta unsettemp <key:word> [context:text]".into(),
        "meta addprefix <priority:integer> <prefix:word> [context:text]".into(),
        "meta addsuffix <priority:integer> <suffix:word> [context:text]".into(),
        "meta setprefix <prefix:word> [context:text]".into(),
        "meta setprefix <priority:integer> <prefix:word> [context:text]".into(),
        "meta setsuffix <suffix:word> [context:text]".into(),
        "meta setsuffix <priority:integer> <suffix:word> [context:text]".into(),
        "meta removeprefix <priority:integer> [prefix:word] [context:text]".into(),
        "meta removesuffix <priority:integer> [suffix:word] [context:text]".into(),
        "meta removetempprefix <priority:integer> [prefix:word] [context:text]".into(),
        "meta removetempsuffix <priority:integer> [suffix:word] [context:text]".into(),
        "meta clear [context:text]".into(),
    ];
    for kind in ["prefix", "suffix", "meta", "chat", "any", "all"] {
        shared.push(format!("meta clear {kind} [context:text]"));
    }
    temp("permission settemp <node:word> true", &mut shared);
    temp("permission settemp <node:word> false", &mut shared);
    temp("parent addtemp <group:word>", &mut shared);
    temp("meta settemp <key:word> <value:word>", &mut shared);
    temp("meta addtempprefix <priority:integer> <prefix:word>", &mut shared);
    temp("meta addtempsuffix <priority:integer> <suffix:word>", &mut shared);
    temp("meta settempprefix <prefix:word>", &mut shared);
    temp("meta settempprefix <priority:integer> <prefix:word>", &mut shared);
    temp("meta settempsuffix <suffix:word>", &mut shared);
    temp("meta settempsuffix <priority:integer> <suffix:word>", &mut shared);
    for holder in ["user <user:player_name>", "group <group:word>"] {
        usages.push(holder.to_owned());
        for u in &shared {
            usages.push(format!("{holder} {u}"));
        }
    }
    // Only a user has tracks to promote along, a switch of primary group and a clone to a user.
    for u in [
        "promote <track:word>",
        "promote <track:word> [context:text]",
        "demote <track:word>",
        "demote <track:word> [context:text]",
        "clone <target:player_name>",
        "parent switchprimarygroup <group:word>",
    ] {
        usages.push(format!("user <user:player_name> {u}"));
    }
    // Only a group has members, a weight, a display name, a rename and a clone to a group.
    for u in ["listmembers [page:integer]", "setweight <weight:integer>", "setdisplayname <name:text>", "rename <name:word>", "rename <name:word> --update-parent-lists", "clone <name:word>"] {
        usages.push(format!("group <group:word> {u}"));
    }

    for root in horizon_luckperms_core::command::ROOTS {
        commands.push(crate::game::native_server::NativeCommand::new(root));
        // Full tree only on the two main roots; the other aliases get the free-text fallback, so the
        // command packet every player receives stays small.
        if *root == "lp" || *root == "luckperms" {
            for u in &usages {
                commands.push(crate::game::native_server::NativeCommand::new(&format!("{root} {u}")));
            }
        }
        commands.push(crate::game::native_server::NativeCommand::new(&format!("{root} [args:text]")));
    }
}
