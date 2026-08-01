use crate::postgres::model::PostgresPsqlDirective;

pub(super) struct PreparedSql {
    pub lint_sql: String,
    pub directives: Vec<PostgresPsqlDirective>,
}

pub(super) fn prepare(source: &str) -> PreparedSql {
    let mut lint_sql = String::with_capacity(source.len());
    let mut directives = Vec::new();

    for (index, line) in source.split_inclusive('\n').enumerate() {
        if let Some(command) = psql_command(line) {
            directives.push(PostgresPsqlDirective {
                command: command.to_string(),
                line: u32::try_from(index + 1).unwrap_or(u32::MAX),
            });
            preserve_line_ending(line, &mut lint_sql);
        } else {
            lint_sql.push_str(line);
        }
    }

    if source.is_empty() {
        lint_sql.clear();
    }

    // Squawk parses SQL rather than the psql client language. Directives are
    // blanked here; source collection and live replay own their semantics.
    PreparedSql {
        lint_sql,
        directives,
    }
}

fn psql_command(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let directive = trimmed.strip_prefix('\\')?;
    let command = directive
        .split(|character: char| character.is_whitespace())
        .next()
        .unwrap_or_default();
    (!command.is_empty()).then_some(command)
}

fn preserve_line_ending(source: &str, target: &mut String) {
    if source.ends_with("\r\n") {
        target.push_str("\r\n");
    } else if source.ends_with('\n') {
        target.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::prepare;

    #[test]
    fn removes_psql_commands_without_shifting_sql_lines_or_retaining_arguments() {
        let prepared = prepare(
            "\\connect postgresql://user:secret@example/db\nCREATE TABLE demo(id int);\n\\gexec",
        );

        assert_eq!(prepared.lint_sql, "\nCREATE TABLE demo(id int);\n");
        assert_eq!(prepared.directives.len(), 2);
        assert_eq!(prepared.directives[0].command, "connect");
        assert_eq!(prepared.directives[0].line, 1);
        assert_eq!(prepared.directives[1].command, "gexec");
        assert!(!format!("{:?}", prepared.directives).contains("secret"));
    }
}
