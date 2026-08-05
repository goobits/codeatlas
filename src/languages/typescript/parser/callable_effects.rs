use super::format::expression_name;
use crate::domain::{CallableEffect, EffectKind, EvidenceClass};
use crate::languages::effects::has_qualified_action;
use std::collections::BTreeSet;
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

pub(super) fn collect_direct_effects(block: &BlockStmt) -> BTreeSet<CallableEffect> {
    let mut collector = EcmaScriptEffectCollector::default();
    block.visit_with(&mut collector);
    collector.effects
}

pub(super) fn collect_arrow_direct_effects(arrow: &ArrowExpr) -> BTreeSet<CallableEffect> {
    let mut collector = EcmaScriptEffectCollector::default();
    arrow.body.visit_with(&mut collector);
    collector.effects
}

#[derive(Default)]
struct EcmaScriptEffectCollector {
    effects: BTreeSet<CallableEffect>,
}

impl EcmaScriptEffectCollector {
    fn record(&mut self, kind: EffectKind) {
        self.effects.insert(CallableEffect::new_direct(
            kind,
            EvidenceClass::BoundaryLimited,
            None,
        ));
    }

    fn record_call(&mut self, path: &str) {
        if has_qualified_action(
            path,
            ".",
            &["fs", "fs.promises", "Deno"],
            &[
                "access",
                "lstat",
                "open",
                "readDir",
                "readFile",
                "readFileSync",
                "readTextFile",
                "realpath",
                "stat",
            ],
        ) {
            self.record(EffectKind::FilesystemRead);
        }
        if has_qualified_action(
            path,
            ".",
            &["fs", "fs.promises", "Deno"],
            &[
                "appendFile",
                "chmod",
                "copyFile",
                "createWriteStream",
                "mkdir",
                "rename",
                "rm",
                "rmdir",
                "symlink",
                "truncate",
                "unlink",
                "writeFile",
                "writeFileSync",
                "writeTextFile",
            ],
        ) {
            self.record(EffectKind::FilesystemWrite);
        }
        if matches!(path, "fetch" | "globalThis.fetch")
            || has_qualified_action(
                path,
                ".",
                &[
                    "axios",
                    "http",
                    "https",
                    "net",
                    "undici",
                    "WebSocket",
                    "XMLHttpRequest",
                ],
                &[
                    "connect", "delete", "get", "open", "patch", "post", "put", "request", "send",
                ],
            )
        {
            self.record(EffectKind::Network);
        }
        if has_qualified_action(
            path,
            ".",
            &[
                "mysql",
                "pg",
                "prisma",
                "redis",
                "sequelize",
                "sqlite",
                "sqlite3",
            ],
            &["connect", "createClient", "createConnection", "open"],
        ) {
            self.record(EffectKind::Database);
        }
        if has_qualified_action(
            path,
            ".",
            &["child_process", "Deno", "process", "worker_threads"],
            &["abort", "exec", "execFile", "exit", "fork", "spawn"],
        ) {
            self.record(EffectKind::Process);
        }
        if has_qualified_action(
            path,
            ".",
            &["Deno.env", "process.env"],
            &["delete", "get", "has", "set", "toObject"],
        ) {
            self.record(EffectKind::Environment);
        }
        if has_qualified_action(path, ".", &["Date", "performance"], &["now"]) {
            self.record(EffectKind::Time);
        }
        if has_qualified_action(
            path,
            ".",
            &["crypto", "Math"],
            &[
                "getRandomValues",
                "random",
                "randomBytes",
                "randomInt",
                "randomUUID",
            ],
        ) {
            self.record(EffectKind::Randomness);
        }
        if matches!(path, "alert" | "confirm" | "prompt")
            || path.starts_with("console.")
            || path.starts_with("localStorage.")
            || path.starts_with("sessionStorage.")
        {
            self.record(EffectKind::AmbientState);
        }
    }
}

impl Visit for EcmaScriptEffectCollector {
    fn visit_call_expr(&mut self, call: &CallExpr) {
        if let Callee::Expr(callee) = &call.callee {
            if let Some(path) = expression_name(callee) {
                self.record_call(&path);
            }
        }
        call.visit_children_with(self);
    }

    fn visit_member_expr(&mut self, member: &MemberExpr) {
        if expression_name(&Expr::Member(member.clone())).is_some_and(|path| {
            path == "process.env" || path == "globalThis.process.env" || path == "Deno.env"
        }) {
            self.record(EffectKind::Environment);
        }
        member.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, expression: &NewExpr) {
        if let Some(path) = expression_name(&expression.callee) {
            if path == "Date" {
                self.record(EffectKind::Time);
            }
            if matches!(path.as_str(), "WebSocket" | "XMLHttpRequest") {
                self.record(EffectKind::Network);
            }
            if matches!(path.as_str(), "pg.Client" | "sqlite3.Database") {
                self.record(EffectKind::Database);
            }
        }
        expression.visit_children_with(self);
    }

    fn visit_function(&mut self, _function: &Function) {}

    fn visit_arrow_expr(&mut self, _arrow: &ArrowExpr) {}

    fn visit_class(&mut self, _class: &Class) {}
}
