//! The usage messages: the command list, a parent command's sub command list, and a command's detailed usage
//! (`CommandSpec` texts from the original's `luckperms_en.properties`).

use super::specs::{spec, Spec};
use super::tables::Child;
use super::Ctx;
use crate::lang::{self, tr};

/// `Argument.asPrettyString`
pub fn pretty_arg(name: &str, required: bool) -> String {
    if required {
        format!("\u{a7}8<\u{a7}7{name}\u{a7}8>")
    } else {
        format!("\u{a7}8[\u{a7}7{name}\u{a7}8]")
    }
}

fn args_line(s: &Spec) -> String {
    s.args.iter().map(|a| pretty_arg(a.name, a.required)).collect::<Vec<_>>().join(" ")
}

impl Ctx<'_> {
    /// `ChildCommand.sendUsage`: `> name - <arg> [arg]`, not prefixed.
    pub fn usage_line(&mut self, name: &str, spec_key: &str) {
        let s = spec(spec_key);
        let mut line = format!("\u{a7}3> \u{a7}a{}", name.to_lowercase());
        if !s.args.is_empty() {
            line.push_str(&format!("\u{a7}3 - {}", args_line(s)));
        }
        self.raw(line);
    }

    /// `Command.sendDetailedUsage`
    pub fn detailed_usage(&mut self, name: &str, spec_key: &str) {
        let s = spec(spec_key);
        self.msg(format!("\u{a7}3\u{a7}l{}\u{a7}3 - \u{a7}b{name}", lang::T_COMMANDSYSTEM_USAGE_USAGE_HEADER));
        self.msg(format!("\u{a7}b> \u{a7}7{}", lang::usage_text(&format!("{spec_key}.description"))));
        if !s.args.is_empty() {
            self.msg(format!("\u{a7}3{}:", lang::T_COMMANDSYSTEM_USAGE_ARGUMENTS_HEADER));
            for a in s.args {
                let desc = lang::usage_text(&format!("{spec_key}.argument.{}", a.id));
                self.msg(format!("\u{a7}b- {}\u{a7}3 -> \u{a7}7{desc}", pretty_arg(a.name, a.required)));
            }
        }
    }

    /// `ParentCommand.sendUsage` / `GenericParentCommand.sendUsageDetailed`: the header and the sub commands the
    /// sender may use; no permission for any of them answers "no permission".
    pub fn parent_usage(&mut self, name: &str, usage: &str, subs: &[(&str, &str)]) {
        if subs.is_empty() {
            self.no_permission();
            return;
        }
        self.msg(format!("\u{a7}b{name} {}: \u{a7}7({usage} ...)", lang::T_COMMANDSYSTEM_USAGE_SUB_COMMANDS_HEADER));
        for (n, key) in subs {
            self.usage_line(n, key);
        }
    }

    /// `Command.sendDetailedUsage` of a holder's child.
    pub fn child_detailed(&mut self, ch: &Child) {
        self.detailed_usage(ch.name, ch.spec);
    }

    /// `CommandManager.sendCommandUsage`: the banner and every command the sender may use.
    pub fn command_usage(&mut self, mains: &[(&str, String)]) {
        self.banner();
        for (_, usage) in mains {
            self.raw(format!("\u{a7}3> \u{a7}a{usage}"));
        }
    }

    /// `Running LuckPerms v5.x.`
    pub fn banner(&mut self) {
        self.msg(format!("\u{a7}2Running \u{a7}bLuckPerms\u{a7}2 \u{a7}bv{}\u{a7}2.", super::LUCKPERMS_VERSION));
    }

    /// `Message.NO_PERMISSION_FOR_SUBCOMMANDS`
    pub fn no_permission_for_subcommands(&mut self) {
        self.msg(format!("{}.", tr(lang::T_COMMANDSYSTEM_NO_PERMISSION_SUBCOMMANDS, '3', &[])));
    }

    /// `Message.FIRST_TIME_SETUP`
    pub fn first_time_setup(&mut self, label: &str, username: &str) {
        self.msg(tr(lang::T_FIRST_TIME_NO_PERMISSIONS_SETUP, '3', &[]));
        self.msg(format!("{}.", tr(lang::T_FIRST_TIME_USE_CONSOLE_TO_GIVE_ACCESS, '3', &[])));
        self.msg(format!("{}:", tr(lang::T_FIRST_TIME_CONSOLE_COMMAND_PROMPT, '3', &[])));
        let cmd = format!("/{label} user {username} permission set luckperms.* true");
        let cmd = crate::text::rich(&format!("\u{a7}a{cmd}"), Some(crate::text::Click::SuggestCommand(cmd.clone())), Some("\u{a7}7Click to suggest the command"));
        self.msg(format!("\u{a7}3 \u{a7}3\u{a7}l>\u{a7}3 {cmd}"));
        self.out.lines.push(String::new());
        self.msg(format!("{}.", tr(lang::T_FIRST_TIME_NEXT_STEP, '3', &[])));
        self.msg(tr(lang::T_FIRST_TIME_WIKI_PROMPT, '3', &[crate::text::rich("\u{a7}7https://luckperms.net/wiki/Usage", Some(crate::text::Click::OpenUrl("https://luckperms.net/wiki/Usage".into())), Some("\u{a7}7Open the wiki"))]));
    }
}
