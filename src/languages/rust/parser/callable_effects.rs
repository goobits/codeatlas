use super::callable::path_identity;
use crate::domain::{CallableEffect, EffectKind, EvidenceClass};
use crate::languages::effects::has_qualified_action;
use std::collections::BTreeSet;
use syn::visit::Visit;

pub(super) fn collect_direct_effects(block: Option<&syn::Block>) -> BTreeSet<CallableEffect> {
    let Some(block) = block else {
        return BTreeSet::new();
    };
    let mut collector = RustEffectCollector::default();
    collector.visit_block(block);
    collector.effects
}

#[derive(Default)]
struct RustEffectCollector {
    effects: BTreeSet<CallableEffect>,
}

impl RustEffectCollector {
    fn record(&mut self, kind: EffectKind) {
        self.effects.insert(CallableEffect::new_direct(
            kind,
            EvidenceClass::BoundaryLimited,
            None,
        ));
    }

    fn record_call(&mut self, path: &str) {
        let action = path.rsplit("::").next().unwrap_or(path);
        if matches!(path, "std::fs::copy" | "tokio::fs::copy") {
            self.record(EffectKind::FilesystemRead);
            self.record(EffectKind::FilesystemWrite);
            return;
        }
        if has_qualified_action(
            path,
            "::",
            &["std::fs", "tokio::fs"],
            &[
                "read",
                "read_dir",
                "read_link",
                "read_to_string",
                "canonicalize",
                "metadata",
                "symlink_metadata",
                "open",
            ],
        ) {
            self.record(EffectKind::FilesystemRead);
        }
        if has_qualified_action(
            path,
            "::",
            &["std::fs", "tokio::fs"],
            &[
                "write",
                "create",
                "create_dir",
                "create_dir_all",
                "hard_link",
                "remove_dir",
                "remove_dir_all",
                "remove_file",
                "rename",
                "set_permissions",
            ],
        ) {
            self.record(EffectKind::FilesystemWrite);
        }
        if has_qualified_action(
            path,
            "::",
            &["std::net", "tokio::net", "reqwest", "hyper", "ureq"],
            &[
                "bind", "connect", "get", "post", "put", "delete", "patch", "request", "send",
            ],
        ) {
            self.record(EffectKind::Network);
        }
        if has_qualified_action(
            path,
            "::",
            &[
                "diesel",
                "mongodb",
                "postgres",
                "redis",
                "rusqlite",
                "sqlx",
                "tokio_postgres",
            ],
            &["connect", "open"],
        ) {
            self.record(EffectKind::Database);
        }
        if has_qualified_action(
            path,
            "::",
            &[
                "std::process",
                "std::thread",
                "tokio::process",
                "tokio::task",
                "tokio",
            ],
            &["abort", "exit", "spawn", "spawn_blocking"],
        ) {
            self.record(EffectKind::Process);
        }
        if path.starts_with("std::env::") {
            self.record(EffectKind::Environment);
        }
        if has_qualified_action(
            path,
            "::",
            &["chrono", "std::time", "tokio::time"],
            &["elapsed", "now", "sleep", "sleep_until", "timeout"],
        ) {
            self.record(EffectKind::Time);
        }
        if has_qualified_action(
            path,
            "::",
            &["getrandom", "rand", "rand_core"],
            &[
                "fill",
                "getrandom",
                "random",
                "random_range",
                "rng",
                "thread_rng",
            ],
        ) {
            self.record(EffectKind::Randomness);
        }
        if matches!(
            path,
            "std::io::stdin" | "std::io::stdout" | "std::io::stderr"
        ) || (path.starts_with("std::sync::")
            && matches!(action, "get_or_init" | "set" | "take"))
        {
            self.record(EffectKind::AmbientState);
        }
    }
}

impl<'ast> Visit<'ast> for RustEffectCollector {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            if path.qself.is_none() {
                self.record_call(&path_identity(&path.path));
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_closure(&mut self, _closure: &'ast syn::ExprClosure) {}

    fn visit_item_fn(&mut self, _function: &'ast syn::ItemFn) {}
}
