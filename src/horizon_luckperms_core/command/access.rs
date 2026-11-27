//! `argument-based-command-permissions` and `require-sender-group-membership-to-modify`
//! (`ArgumentPermissions`): who may view or modify which user, group or track, and use which context.
//! Each check answers true when the sender is DENIED, as in LuckPerms.

use super::Ctx;
use crate::horizon_luckperms_core::calc::Tristate;
use crate::horizon_luckperms_core::context::ContextSet;

/// What a command acts on.
pub(crate) enum Target<'a> {
    User(u128),
    Group(&'a str),
    Track(&'a str),
}

impl Ctx<'_> {
    /// `Sender.getPermissionValue`: the console has everything.
    fn value(&self, node: &str) -> Tristate {
        if self.sender.console {
            Tristate::True
        } else {
            self.rt.check(self.sender.uuid, node)
        }
    }

    /// The specific node decides when set; the global one decides otherwise (an unset global node denies).
    fn specific_or_global(&self, specific: &str, global: &str) -> bool {
        match self.value(specific) {
            Tristate::Undefined => self.value(global) != Tristate::True,
            state => state != Tristate::True,
        }
    }

    fn deny_target(&self, base: &str, kind: &str, target: &Target<'_>) -> bool {
        if !self.cfg.argument_based_command_permissions {
            return false;
        }
        match target {
            Target::User(uuid) => {
                let who = if *uuid == self.sender.uuid { "self" } else { "others" };
                self.specific_or_global(&format!("{base}.{kind}.{who}"), &format!("luckperms.{kind}.user.{who}"))
            }
            Target::Group(name) => self.specific_or_global(&format!("{base}.{kind}.{name}"), &format!("luckperms.{kind}.group.{name}")),
            Target::Track(name) => self.specific_or_global(&format!("{base}.{kind}.{name}"), &format!("luckperms.{kind}.track.{name}")),
        }
    }

    /// `checkModifyPerms`
    pub(crate) fn deny_modify(&self, base: &str, target: &Target<'_>) -> bool {
        self.deny_target(base, "modify", target)
    }

    /// `checkViewPerms`
    pub(crate) fn deny_view(&self, base: &str, target: &Target<'_>) -> bool {
        self.deny_target(base, "view", target)
    }

    /// `checkArguments`: `<base>.<arg>.<arg>...` must be granted.
    pub(crate) fn deny_arguments(&self, base: &str, args: &[&str]) -> bool {
        if !self.cfg.argument_based_command_permissions || args.is_empty() {
            return false;
        }
        let mut node = base.to_owned();
        for a in args {
            node.push('.');
            node.push_str(a);
        }
        self.value(&node) != Tristate::True
    }

    /// `checkContext`
    pub(crate) fn deny_context(&self, base: &str, contexts: &ContextSet) -> bool {
        if !self.cfg.argument_based_command_permissions {
            return false;
        }
        if contexts.is_empty() {
            return self.specific_or_global(&format!("{base}.usecontext.global"), "luckperms.usecontext.global");
        }
        for (k, v) in contexts.pairs() {
            let state = match self.value(&format!("{base}.usecontext.{k}.{v}")) {
                Tristate::Undefined => self.value(&format!("luckperms.usecontext.{k}.{v}")),
                s => s,
            };
            if state == Tristate::False {
                return true;
            }
        }
        false
    }

    /// `checkGroup`: with `require-sender-group-membership-to-modify`, a player may only change a group they
    /// are a direct member of (`group.<name>` set true on themselves).
    pub(crate) fn deny_group(&self, group: &str) -> bool {
        if !self.cfg.require_sender_group_membership || self.sender.console {
            return false;
        }
        let node = format!("group.{group}");
        !(self.rt.check(self.sender.uuid, &node) == Tristate::True && self.rt.check_processor(self.sender.uuid, &node) == "direct")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_set_pairs_are_visible() {
        let mut c = ContextSet::new();
        c.add("server", "rec");
        assert_eq!(c.pairs().collect::<Vec<_>>(), vec![("server", "rec")]);
    }
}
