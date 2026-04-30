//! Per-database driver/run-mode/port catalog.
//!
//! Sister module to [`registry`](crate::registry) — kept separate because the
//! shape (drivers grouped *by language*, plus per-DB attributes like
//! `default_port` / `supports_run_mode`) doesn't fit `LanguageSpec`. Adding a
//! new database is one `DatabaseSpec` const + one arm in [`spec_for`].

use crate::{Database, Language};

/// Static description of a database choice. `None` means the database doesn't
/// expose that knob (SQLite has no port and no run mode; the `Database::None`
/// variant has nothing).
pub struct DatabaseSpec {
    /// Default port to display when the user leaves the port input empty.
    /// `None` for embedded databases (SQLite) and the `None` variant.
    pub default_port: Option<u16>,
    /// Whether the database asks the user how it should be run (Docker /
    /// Native / Managed). `false` for SQLite (always embedded) and `None`.
    pub supports_run_mode: bool,
    /// Per-language driver catalogs. Languages not represented here have no
    /// drivers for this DB; the handler also filters this list down to the
    /// languages the user actually selected upstream.
    pub driver_groups: &'static [DriverGroup],
}

pub struct DriverGroup {
    pub language: Language,
    pub drivers: &'static [Driver],
}

pub struct Driver {
    /// Stable identifier — typically the canonical PyPI / crates.io name.
    /// Pairs with [`Language`] in [`crate::DatabaseConfig::drivers`] to avoid
    /// cross-ecosystem ID collisions (e.g. Python `redis` vs Rust `redis`).
    pub id: &'static str,
    pub label: &'static str,
}

pub const EMPTY_DB_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: None,
    supports_run_mode: false,
    driver_groups: &[],
};

pub fn spec_for(database: Database) -> &'static DatabaseSpec {
    match database {
        Database::PostgreSQL => &POSTGRESQL_SPEC,
        Database::MySQL => &MYSQL_SPEC,
        Database::SQLite => &SQLITE_SPEC,
        Database::MongoDB => &MONGODB_SPEC,
        Database::Redis => &REDIS_SPEC,
        Database::None => &EMPTY_DB_SPEC,
    }
}

/// Look up a driver record by `(language, id)`. Walks every database's
/// catalog, so the same `(Python, "sqlalchemy")` pair resolves regardless
/// of which DB the user is configuring. Returns `None` for unknown IDs.
pub fn driver_by_id(language: Language, id: &str) -> Option<&'static Driver> {
    for db in [
        Database::PostgreSQL,
        Database::MySQL,
        Database::SQLite,
        Database::MongoDB,
        Database::Redis,
    ] {
        for group in spec_for(db).driver_groups {
            if group.language != language {
                continue;
            }
            for driver in group.drivers {
                if driver.id == id {
                    return Some(driver);
                }
            }
        }
    }
    None
}

// --- PostgreSQL ---

const POSTGRESQL_PYTHON: &[Driver] = &[
    Driver { id: "psycopg",     label: "psycopg" },
    Driver { id: "psycopg2",    label: "psycopg2" },
    Driver { id: "asyncpg",     label: "asyncpg" },
    Driver { id: "sqlalchemy",  label: "SQLAlchemy" },
];
const POSTGRESQL_RUST: &[Driver] = &[
    Driver { id: "sqlx",            label: "sqlx" },
    Driver { id: "diesel",          label: "diesel" },
    Driver { id: "tokio-postgres",  label: "tokio-postgres" },
    Driver { id: "postgres",        label: "postgres" },
];
const POSTGRESQL_GROUPS: &[DriverGroup] = &[
    DriverGroup { language: Language::Python, drivers: POSTGRESQL_PYTHON },
    DriverGroup { language: Language::Rust,   drivers: POSTGRESQL_RUST },
];
pub const POSTGRESQL_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: Some(5432),
    supports_run_mode: true,
    driver_groups: POSTGRESQL_GROUPS,
};

// --- MySQL ---

const MYSQL_PYTHON: &[Driver] = &[
    Driver { id: "mysqlclient",            label: "mysqlclient" },
    Driver { id: "pymysql",                label: "PyMySQL" },
    Driver { id: "mysql-connector-python", label: "mysql-connector-python" },
    Driver { id: "sqlalchemy",             label: "SQLAlchemy" },
];
const MYSQL_RUST: &[Driver] = &[
    Driver { id: "sqlx",         label: "sqlx" },
    Driver { id: "diesel",       label: "diesel" },
    Driver { id: "mysql_async",  label: "mysql_async" },
    Driver { id: "mysql",        label: "mysql" },
];
const MYSQL_GROUPS: &[DriverGroup] = &[
    DriverGroup { language: Language::Python, drivers: MYSQL_PYTHON },
    DriverGroup { language: Language::Rust,   drivers: MYSQL_RUST },
];
pub const MYSQL_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: Some(3306),
    supports_run_mode: true,
    driver_groups: MYSQL_GROUPS,
};

// --- SQLite ---

const SQLITE_PYTHON: &[Driver] = &[
    Driver { id: "sqlite3",     label: "sqlite3 (stdlib)" },
    Driver { id: "sqlalchemy",  label: "SQLAlchemy" },
];
const SQLITE_RUST: &[Driver] = &[
    Driver { id: "rusqlite",  label: "rusqlite" },
    Driver { id: "sqlx",      label: "sqlx" },
    Driver { id: "diesel",    label: "diesel" },
];
const SQLITE_GROUPS: &[DriverGroup] = &[
    DriverGroup { language: Language::Python, drivers: SQLITE_PYTHON },
    DriverGroup { language: Language::Rust,   drivers: SQLITE_RUST },
];
pub const SQLITE_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: None,
    supports_run_mode: false,
    driver_groups: SQLITE_GROUPS,
};

// --- MongoDB ---

const MONGODB_PYTHON: &[Driver] = &[
    Driver { id: "pymongo", label: "pymongo" },
    Driver { id: "motor",   label: "motor" },
    Driver { id: "beanie",  label: "beanie" },
];
const MONGODB_RUST: &[Driver] = &[
    Driver { id: "mongodb", label: "mongodb" },
];
const MONGODB_GROUPS: &[DriverGroup] = &[
    DriverGroup { language: Language::Python, drivers: MONGODB_PYTHON },
    DriverGroup { language: Language::Rust,   drivers: MONGODB_RUST },
];
pub const MONGODB_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: Some(27017),
    supports_run_mode: true,
    driver_groups: MONGODB_GROUPS,
};

// --- Redis ---

const REDIS_PYTHON: &[Driver] = &[
    Driver { id: "redis",   label: "redis" },
    Driver { id: "hiredis", label: "hiredis" },
];
const REDIS_RUST: &[Driver] = &[
    Driver { id: "redis", label: "redis" },
    Driver { id: "fred",  label: "fred" },
];
const REDIS_GROUPS: &[DriverGroup] = &[
    DriverGroup { language: Language::Python, drivers: REDIS_PYTHON },
    DriverGroup { language: Language::Rust,   drivers: REDIS_RUST },
];
pub const REDIS_SPEC: DatabaseSpec = DatabaseSpec {
    default_port: Some(6379),
    supports_run_mode: true,
    driver_groups: REDIS_GROUPS,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn spec_for_postgresql_populated() {
        let s = spec_for(Database::PostgreSQL);
        assert_eq!(s.default_port, Some(5432));
        assert!(s.supports_run_mode);
        assert!(!s.driver_groups.is_empty());
    }

    #[test]
    fn spec_for_sqlite_omits_port_and_run_mode() {
        let s = spec_for(Database::SQLite);
        assert!(s.default_port.is_none());
        assert!(!s.supports_run_mode);
        assert!(!s.driver_groups.is_empty());
    }

    #[test]
    fn spec_for_none_is_empty() {
        let s = spec_for(Database::None);
        assert!(s.default_port.is_none());
        assert!(!s.supports_run_mode);
        assert!(s.driver_groups.is_empty());
    }

    #[test]
    fn server_dbs_have_well_known_ports() {
        assert_eq!(spec_for(Database::PostgreSQL).default_port, Some(5432));
        assert_eq!(spec_for(Database::MySQL).default_port, Some(3306));
        assert_eq!(spec_for(Database::MongoDB).default_port, Some(27017));
        assert_eq!(spec_for(Database::Redis).default_port, Some(6379));
    }

    #[test]
    fn server_dbs_support_run_mode() {
        for db in [
            Database::PostgreSQL,
            Database::MySQL,
            Database::MongoDB,
            Database::Redis,
        ] {
            assert!(spec_for(db).supports_run_mode, "{db} should support run mode");
        }
    }

    #[test]
    fn driver_ids_unique_within_each_group() {
        for db in [
            Database::PostgreSQL,
            Database::MySQL,
            Database::SQLite,
            Database::MongoDB,
            Database::Redis,
        ] {
            for group in spec_for(db).driver_groups {
                let mut seen = HashSet::new();
                for driver in group.drivers {
                    assert!(
                        seen.insert(driver.id),
                        "duplicate driver id '{}' in {db} / {}",
                        driver.id,
                        group.language,
                    );
                }
            }
        }
    }

    #[test]
    fn driver_by_id_finds_known_drivers() {
        let psy = driver_by_id(Language::Python, "psycopg").expect("psycopg listed");
        assert_eq!(psy.label, "psycopg");
        let sqlx = driver_by_id(Language::Rust, "sqlx").expect("sqlx listed");
        assert_eq!(sqlx.label, "sqlx");
    }

    #[test]
    fn driver_by_id_distinguishes_languages() {
        // "redis" is a valid id in BOTH the Python and Rust catalogs;
        // driver_by_id must return the per-language record.
        let py = driver_by_id(Language::Python, "redis");
        let rs = driver_by_id(Language::Rust, "redis");
        assert!(py.is_some());
        assert!(rs.is_some());
    }

    #[test]
    fn driver_by_id_returns_none_for_unknown() {
        assert!(driver_by_id(Language::Python, "nonexistent").is_none());
        assert!(driver_by_id(Language::Go, "psycopg").is_none());
    }

    #[test]
    fn no_groups_for_unsupported_languages() {
        // First-iteration scope is Python + Rust; everything else has no group.
        for db in [
            Database::PostgreSQL,
            Database::MySQL,
            Database::SQLite,
            Database::MongoDB,
            Database::Redis,
        ] {
            for group in spec_for(db).driver_groups {
                assert!(
                    matches!(group.language, Language::Python | Language::Rust),
                    "{db} should only carry Python/Rust groups, found {}",
                    group.language,
                );
            }
        }
    }
}
