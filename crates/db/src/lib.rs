//! SQLite storage for Saffron: connection pool, embedded migrations, and the column
//! encodings shared by every query.

pub mod audit;
pub mod decimal;
pub mod instruments;
pub mod login_throttle;
pub mod passkeys;
pub mod portfolios;
pub mod prices;
pub mod sessions;
pub mod settings;
pub mod time;
pub mod users;

use std::{path::Path, time::Duration};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use thiserror::Error;

pub use sqlx::SqlitePool as Pool;

/// File name of the database inside the data directory.
pub const DATABASE_FILE: &str = "saffron.db";

/// Storage failures.
#[derive(Debug, Error)]
pub enum StorageError {
    /// The database rejected an operation.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// An embedded migration failed.
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    /// The data directory could not be created.
    #[error("cannot create the data directory `{path}`: {source}")]
    CreateDir {
        /// Directory that could not be created.
        path: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The database file could not be created.
    #[error("cannot create the database file `{path}`: {source}")]
    CreateFile {
        /// File that could not be created.
        path: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// File permissions could not be restricted.
    #[error("cannot restrict permissions on `{path}`: {source}")]
    Permissions {
        /// File or directory whose mode could not be set.
        path: String,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
}

/// Whether `error` is a UNIQUE or PRIMARY KEY constraint violation.
#[must_use]
pub fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.is_unique_violation())
}

/// Opens the database at `path`, creating it and its directory when missing, and runs
/// pending migrations.
///
/// A directory this process creates is private (`0700`), and the database file is made
/// owner-only (`0600`) before migrations write anything: it holds financial data and
/// session hashes.
pub async fn open(path: &Path) -> Result<SqlitePool, StorageError> {
    if let Some(parent) = data_directory(path) {
        let create_dir_error = |source| StorageError::CreateDir {
            path: parent.display().to_string(),
            source,
        };
        let existed = tokio::fs::try_exists(parent)
            .await
            .map_err(create_dir_error)?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(create_dir_error)?;
        if !existed {
            restrict(parent, 0o700).await?;
        }
    }

    // SQLite gives the WAL and SHM sidecars the mode of the main file, so the file
    // must be private before the first connection opens it.
    create_private_file(path).await?;
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connect_options().filename(path))
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Opens a single-connection in-memory database with migrations applied, for tests.
pub async fn open_in_memory() -> Result<SqlitePool, StorageError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options().in_memory(true))
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

/// Runs every embedded migration that has not run yet.
pub async fn migrate(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

fn connect_options() -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
}

/// The directory holding `path`, or `None` for a bare file name (the working
/// directory, which this process neither creates nor restricts).
fn data_directory(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

/// Creates `path` as an empty owner-only file when missing, and restricts it when it
/// already exists.
async fn create_private_file(path: &Path) -> Result<(), StorageError> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .await
        .map_err(|source| StorageError::CreateFile {
            path: path.display().to_string(),
            source,
        })?;
    restrict(path, 0o600).await
}

#[cfg(unix)]
async fn restrict(path: &Path, mode: u32) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .await
        .map_err(|source| StorageError::Permissions {
            path: path.display().to_string(),
            source,
        })
}

#[cfg(not(unix))]
#[allow(clippy::unused_async)]
async fn restrict(_path: &Path, _mode: u32) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_file_name_has_no_data_directory() {
        assert_eq!(data_directory(Path::new("saffron.db")), None);
    }

    #[test]
    fn a_nested_path_has_its_parent_as_data_directory() {
        assert_eq!(
            data_directory(Path::new("data/saffron.db")),
            Some(Path::new("data"))
        );
    }
}
