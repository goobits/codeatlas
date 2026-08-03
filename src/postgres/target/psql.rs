use anyhow::{Context, Result};
use percent_encoding::percent_decode_str;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use url::Url;

const PSQL_ENV: &str = "CODEATLAS_PSQL_PATH";
const MAX_PSQL_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const LIBPQ_ENVIRONMENT: [&str; 15] = [
    "PGAPPNAME",
    "PGCONNECT_TIMEOUT",
    "PGDATABASE",
    "PGHOST",
    "PGOPTIONS",
    "PGPASSFILE",
    "PGPASSWORD",
    "PGPORT",
    "PGSERVICE",
    "PGSERVICEFILE",
    "PGSSLCERT",
    "PGSSLKEY",
    "PGSSLMODE",
    "PGSSLROOTCERT",
    "PGUSER",
];

#[derive(Clone)]
pub(super) struct Connection {
    host: String,
    port: u16,
    user: String,
    password: Option<String>,
    database: String,
    parameters: BTreeMap<String, String>,
}

impl Connection {
    pub(super) fn from_environment(name: &str) -> Result<Self> {
        let value = std::env::var(name)
            .with_context(|| format!("PostgreSQL target requires environment variable {name}"))?;
        let url = Url::parse(&value).with_context(|| {
            format!("PostgreSQL target environment variable {name} is not a URL")
        })?;
        if !matches!(url.scheme(), "postgres" | "postgresql") {
            anyhow::bail!(
                "PostgreSQL target environment variable {name} must use postgres:// or postgresql://"
            );
        }
        if url.fragment().is_some() {
            anyhow::bail!(
                "PostgreSQL target environment variable {name} must not contain a URL fragment"
            );
        }
        let host = decode(
            url.host_str()
                .filter(|host| !host.is_empty())
                .context("PostgreSQL target URL needs a host")?,
            "PostgreSQL host",
        )?;
        let user = decode(url.username(), "PostgreSQL username")?;
        if user.is_empty() {
            anyhow::bail!("PostgreSQL target URL needs an explicit username");
        }
        let password = url
            .password()
            .map(|password| decode(password, "PostgreSQL password"))
            .transpose()?;
        let database = decode(url.path().trim_start_matches('/'), "PostgreSQL database")?;
        if database.is_empty() || database.contains('/') {
            anyhow::bail!("PostgreSQL target URL needs one database path segment");
        }
        let mut parameters = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            let environment = match key.as_ref() {
                "application_name" => "PGAPPNAME",
                "connect_timeout" => "PGCONNECT_TIMEOUT",
                "options" => "PGOPTIONS",
                "sslcert" => "PGSSLCERT",
                "sslkey" => "PGSSLKEY",
                "sslmode" => "PGSSLMODE",
                "sslrootcert" => "PGSSLROOTCERT",
                unsupported => {
                    anyhow::bail!(
                        "Unsupported PostgreSQL target URL parameter {unsupported:?}; CodeAtlas refuses to silently change connection semantics"
                    )
                }
            };
            parameters.insert(environment.to_string(), value.into_owned());
        }
        Ok(Self {
            host,
            port: url.port().unwrap_or(5432),
            user,
            password,
            database,
            parameters,
        })
    }

    pub(super) fn with_database(&self, database: &str) -> Self {
        let mut connection = self.clone();
        connection.database = database.to_string();
        connection
    }

    fn configure(&self, command: &mut Command) {
        for name in LIBPQ_ENVIRONMENT {
            command.env_remove(name);
        }
        command
            .env("PGHOST", &self.host)
            .env("PGPORT", self.port.to_string())
            .env("PGUSER", &self.user)
            .env("PGDATABASE", &self.database)
            .env(
                "PGCONNECT_TIMEOUT",
                self.parameters
                    .get("PGCONNECT_TIMEOUT")
                    .map_or("5", String::as_str),
            );
        if let Some(password) = &self.password {
            command.env("PGPASSWORD", password);
        }
        for (name, value) in &self.parameters {
            command.env(name, value);
        }
    }

    fn secrets(&self) -> impl Iterator<Item = &str> {
        self.password.iter().map(String::as_str)
    }
}

pub(super) struct Psql {
    executable: PathBuf,
}

impl Psql {
    pub(super) fn resolve(explicit: Option<&Path>) -> Result<Self> {
        Ok(Self {
            executable: crate::external_tool::resolve(explicit, PSQL_ENV, "psql", "psql")?,
        })
    }

    pub(super) fn run(
        &self,
        connection: &Connection,
        sql: &str,
        single_transaction: bool,
    ) -> Result<PsqlOutput> {
        let mut command = Command::new(&self.executable);
        command.args([
            "--file=-",
            "--no-align",
            "--no-password",
            "--no-psqlrc",
            "--quiet",
            "--set=ON_ERROR_STOP=1",
            "--tuples-only",
        ]);
        if single_transaction {
            command.arg("--single-transaction");
        }
        connection.configure(&mut command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().with_context(|| {
            format!(
                "Could not start psql at {}. Install PostgreSQL client tools, set {PSQL_ENV}, or pass --psql.",
                self.executable.display()
            )
        })?;
        child
            .stdin
            .take()
            .context("Could not open psql stdin")?
            .write_all(sql.as_bytes())
            .context("Could not write SQL to psql")?;
        let output = child.wait_with_output().context("psql did not complete")?;
        if output.stdout.len() > MAX_PSQL_OUTPUT_BYTES
            || output.stderr.len() > MAX_PSQL_OUTPUT_BYTES
        {
            anyhow::bail!(
                "psql output exceeded the {} byte safety limit",
                MAX_PSQL_OUTPUT_BYTES
            );
        }
        Ok(PsqlOutput {
            success: output.status.success(),
            stdout: String::from_utf8(output.stdout).context("psql stdout was not UTF-8")?,
            error: safe_error(&output.stderr, connection.secrets()),
        })
    }
}

pub(super) struct PsqlOutput {
    pub success: bool,
    pub stdout: String,
    pub error: String,
}

fn decode(value: &str, label: &str) -> Result<String> {
    percent_decode_str(value)
        .decode_utf8()
        .with_context(|| format!("{label} is not UTF-8"))
        .map(|value| value.into_owned())
}

fn safe_error<'a>(stderr: &[u8], secrets: impl Iterator<Item = &'a str>) -> String {
    let mut message = String::from_utf8_lossy(stderr)
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("ERROR:")
                || line.starts_with("FATAL:")
                || line.starts_with("DETAIL:")
                || line.starts_with("HINT:")
                || line.starts_with("psql:")
        })
        .take(8)
        .collect::<Vec<_>>()
        .join("\n");
    for secret in secrets.filter(|secret| !secret.is_empty()) {
        message = message.replace(secret, "[redacted]");
    }
    if message.is_empty() {
        "psql failed without a safe diagnostic".to_string()
    } else {
        message.chars().take(2_000).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_error, Connection};

    #[test]
    fn connection_urls_are_structural_and_unknown_options_are_rejected() {
        let variable = format!("CODEATLAS_POSTGRES_TEST_URL_{}", std::process::id());
        std::env::set_var(
            &variable,
            "postgresql://user:p%40ss@localhost:5433/postgres?sslmode=disable",
        );
        let connection = Connection::from_environment(&variable).expect("PostgreSQL URL");
        assert_eq!(connection.host, "localhost");
        assert_eq!(connection.port, 5433);
        assert_eq!(connection.user, "user");
        assert_eq!(connection.password.as_deref(), Some("p@ss"));
        assert_eq!(connection.database, "postgres");
        std::env::set_var(
            &variable,
            "postgresql://user@%2Fvar%2Frun%2Fpostgresql/postgres",
        );
        let socket_connection =
            Connection::from_environment(&variable).expect("PostgreSQL socket URL");
        assert_eq!(socket_connection.host, "/var/run/postgresql");
        std::env::set_var(
            &variable,
            "postgresql://user@localhost/postgres?unknown_option=true",
        );
        assert!(Connection::from_environment(&variable).is_err());
        std::env::remove_var(variable);
    }

    #[test]
    fn psql_diagnostics_drop_source_lines_and_redact_passwords() {
        let error = safe_error(
            b"ERROR: relation missing for secret\nLINE 1: SELECT * FROM missing\nHINT: check secret\n",
            ["secret"].into_iter(),
        );
        assert_eq!(
            error,
            "ERROR: relation missing for [redacted]\nHINT: check [redacted]"
        );
        assert!(!error.contains("SELECT"));
    }
}
