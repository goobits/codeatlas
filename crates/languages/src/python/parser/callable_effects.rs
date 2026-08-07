use super::callable::qualified_name;
use crate::effects::{has_qualified_action, record_direct_effect};
use codeatlas_domain::{CallableEffect, EffectKind};
use rustpython_ast::Visitor;
use rustpython_parser::ast;
use std::collections::BTreeSet;

pub(super) fn collect_direct_effects(body: &[ast::Stmt]) -> BTreeSet<CallableEffect> {
    let mut collector = PythonEffectCollector::default();
    for statement in body {
        collector.visit_stmt(statement.clone());
    }
    collector.effects
}

#[derive(Default)]
struct PythonEffectCollector {
    effects: BTreeSet<CallableEffect>,
}

impl PythonEffectCollector {
    fn record(&mut self, kind: EffectKind) {
        record_direct_effect(&mut self.effects, kind);
    }

    fn record_call(&mut self, call: &ast::ExprCall) {
        let Some(path) = qualified_name(&call.func) else {
            return;
        };
        if matches!(path.as_str(), "open" | "io.open" | "pathlib.Path.open") {
            match call.args.get(1) {
                None => self.record(EffectKind::FilesystemRead),
                Some(expression) => match string_literal(expression) {
                    Some(mode) if mode.chars().any(|character| "wax+".contains(character)) => {
                        self.record(EffectKind::FilesystemWrite);
                    }
                    Some(_) => self.record(EffectKind::FilesystemRead),
                    None => {
                        self.record(EffectKind::FilesystemRead);
                        self.record(EffectKind::FilesystemWrite);
                    }
                },
            }
        }
        if has_qualified_action(
            &path,
            ".",
            &["os", "pathlib.Path", "shutil"],
            &[
                "exists",
                "iterdir",
                "listdir",
                "read_bytes",
                "read_text",
                "readlink",
                "scandir",
                "stat",
                "walk",
            ],
        ) {
            self.record(EffectKind::FilesystemRead);
        }
        if has_qualified_action(
            &path,
            ".",
            &["os", "pathlib.Path", "shutil"],
            &[
                "chmod",
                "copy",
                "copy2",
                "copyfile",
                "link",
                "makedirs",
                "mkdir",
                "move",
                "remove",
                "rename",
                "replace",
                "rmdir",
                "symlink",
                "touch",
                "unlink",
                "write_bytes",
                "write_text",
            ],
        ) {
            self.record(EffectKind::FilesystemWrite);
        }
        if has_qualified_action(
            &path,
            ".",
            &[
                "aiohttp",
                "http.client",
                "httpx",
                "requests",
                "socket",
                "urllib.request",
                "websockets",
            ],
            &[
                "bind", "connect", "delete", "get", "open", "patch", "post", "put", "request",
                "send", "urlopen",
            ],
        ) {
            self.record(EffectKind::Network);
        }
        if has_qualified_action(
            &path,
            ".",
            &[
                "asyncpg",
                "mysql.connector",
                "psycopg",
                "psycopg2",
                "pymongo",
                "redis",
                "sqlalchemy",
                "sqlite3",
            ],
            &["connect", "create_engine"],
        ) {
            self.record(EffectKind::Database);
        }
        if path.starts_with("subprocess.")
            || has_qualified_action(
                &path,
                ".",
                &["multiprocessing", "os"],
                &[
                    "execv", "execve", "fork", "popen", "spawnl", "spawnv", "start", "system",
                ],
            )
        {
            self.record(EffectKind::Process);
        }
        if has_qualified_action(&path, ".", &["os"], &["getenv", "putenv", "unsetenv"]) {
            self.record(EffectKind::Environment);
        }
        if has_qualified_action(
            &path,
            ".",
            &["datetime", "time"],
            &[
                "monotonic",
                "monotonic_ns",
                "now",
                "perf_counter",
                "perf_counter_ns",
                "sleep",
                "time",
                "time_ns",
                "today",
                "utcnow",
            ],
        ) {
            self.record(EffectKind::Time);
        }
        if path.starts_with("random.")
            || path.starts_with("secrets.")
            || matches!(path.as_str(), "os.urandom" | "uuid.uuid1" | "uuid.uuid4")
        {
            self.record(EffectKind::Randomness);
        }
        if matches!(
            path.as_str(),
            "input" | "print" | "sys.stdin.read" | "sys.stdout.write" | "sys.stderr.write"
        ) {
            self.record(EffectKind::AmbientState);
        }
    }
}

impl Visitor for PythonEffectCollector {
    fn visit_expr_call(&mut self, node: ast::ExprCall) {
        self.record_call(&node);
        self.generic_visit_expr_call(node);
    }

    fn visit_expr_attribute(&mut self, node: ast::ExprAttribute) {
        if qualified_name(&node.value).is_some_and(|parent| {
            (parent == "os" && node.attr.as_str() == "environ")
                || (parent == "sys" && node.attr.as_str() == "argv")
        }) {
            self.record(EffectKind::Environment);
        }
        self.generic_visit_expr_attribute(node);
    }

    fn visit_stmt_function_def(&mut self, _node: ast::StmtFunctionDef) {}

    fn visit_stmt_async_function_def(&mut self, _node: ast::StmtAsyncFunctionDef) {}

    fn visit_stmt_class_def(&mut self, _node: ast::StmtClassDef) {}

    fn visit_expr_lambda(&mut self, _node: ast::ExprLambda) {}
}

fn string_literal(expression: &ast::Expr) -> Option<&str> {
    match expression {
        ast::Expr::Constant(constant) => match &constant.value {
            ast::Constant::Str(value) => Some(value),
            _ => None,
        },
        _ => None,
    }
}
