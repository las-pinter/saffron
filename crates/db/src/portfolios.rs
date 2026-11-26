//! Portfolios, their owners, and the users they are shared with.

use jiff::Timestamp;
use sqlx::SqlitePool;

use crate::time;

/// One portfolio row.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Portfolio {
    /// Row id.
    pub id: i64,
    /// The user who owns it.
    pub owner_id: i64,
    /// Name, unique per owner.
    pub name: String,
    /// ISO 4217 code that values are reported in.
    pub base_currency: String,
    /// When it was created, as RFC 3339 text.
    pub created_at: String,
    /// When it was archived, as RFC 3339 text; `None` while active.
    pub archived_at: Option<String>,
}

/// What a user may do with a portfolio, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Reads it.
    Viewer,
    /// Reads and changes its contents.
    Editor,
    /// Everything, including renaming, archiving, and sharing it.
    Owner,
}

impl Role {
    /// The API and database name of the role.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Owner => "owner",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "viewer" => Some(Self::Viewer),
            "editor" => Some(Self::Editor),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }
}

/// A role that can be given through sharing; ownership cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareRole {
    /// Reads the portfolio.
    Viewer,
    /// Reads and changes its contents.
    Editor,
}

impl From<ShareRole> for Role {
    fn from(role: ShareRole) -> Self {
        match role {
            ShareRole::Viewer => Self::Viewer,
            ShareRole::Editor => Self::Editor,
        }
    }
}

/// A portfolio together with the role a given user holds on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    /// The portfolio.
    pub portfolio: Portfolio,
    /// The user's role on it.
    pub role: Role,
    /// The owner's sign-in name.
    pub owner_username: String,
    /// The owner's display name.
    pub owner_display_name: String,
}

/// A user a portfolio is shared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Share {
    /// The user's id.
    pub user_id: i64,
    /// The user's sign-in name.
    pub username: String,
    /// The user's display name.
    pub display_name: String,
    /// The role given.
    pub role: Role,
    /// When it was shared, as RFC 3339 text.
    pub created_at: String,
}

/// The fields of a new portfolio.
#[derive(Debug, Clone, Copy)]
pub struct NewPortfolio<'a> {
    /// The owner.
    pub owner_id: i64,
    /// Name, unique per owner.
    pub name: &'a str,
    /// ISO 4217 code, upper case.
    pub base_currency: &'a str,
}

/// Every portfolio column of `p`, then the role of user `?1` on it, then `$tail`.
/// Rows are limited to portfolios that user owns or that are shared with them.
macro_rules! select_access {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.owner_id, p.name, p.base_currency, p.created_at, p.archived_at,
                    CASE WHEN p.owner_id = ?1 THEN 'owner' ELSE s.role END AS role,
                    o.username, o.display_name
             FROM portfolios p
             JOIN users o ON o.id = p.owner_id
             LEFT JOIN portfolio_shares s ON s.portfolio_id = p.id AND s.user_id = ?1
             WHERE (p.owner_id = ?1 OR s.user_id IS NOT NULL) ",
            $tail
        )
    };
}

type AccessRow = (
    i64,
    i64,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);

fn access_from_row(row: AccessRow) -> Result<Access, sqlx::Error> {
    let (
        id,
        owner_id,
        name,
        base_currency,
        created_at,
        archived_at,
        role,
        owner_username,
        owner_display_name,
    ) = row;
    let role = Role::parse(&role)
        .ok_or_else(|| sqlx::Error::Decode(format!("unknown portfolio role `{role}`").into()))?;
    Ok(Access {
        portfolio: Portfolio {
            id,
            owner_id,
            name,
            base_currency,
            created_at,
            archived_at,
        },
        role,
        owner_username,
        owner_display_name,
    })
}

/// Creates a portfolio and returns its id. A name the owner already uses fails with
/// a unique-constraint error; see [`crate::is_unique_violation`].
pub async fn create(
    pool: &SqlitePool,
    portfolio: NewPortfolio<'_>,
    now: Timestamp,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO portfolios (owner_id, name, base_currency, created_at)
         VALUES (?, ?, ?, ?) RETURNING id",
    )
    .bind(portfolio.owner_id)
    .bind(portfolio.name)
    .bind(portfolio.base_currency)
    .bind(time::to_text(now))
    .fetch_one(pool)
    .await
}

/// Portfolio `portfolio_id` with the role of `user_id` on it; `None` when the user
/// neither owns it nor has it shared with them, or when it does not exist.
pub async fn access(
    pool: &SqlitePool,
    user_id: i64,
    portfolio_id: i64,
) -> Result<Option<Access>, sqlx::Error> {
    let row: Option<AccessRow> = sqlx::query_as(select_access!("AND p.id = ?2"))
        .bind(user_id)
        .bind(portfolio_id)
        .fetch_optional(pool)
        .await?;
    row.map(access_from_row).transpose()
}

/// Every portfolio `user_id` owns or has shared with them, by name.
pub async fn visible_to(pool: &SqlitePool, user_id: i64) -> Result<Vec<Access>, sqlx::Error> {
    let rows: Vec<AccessRow> = sqlx::query_as(select_access!("ORDER BY p.name, p.id"))
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(access_from_row).collect()
}

/// Renames a portfolio. A name the owner already uses fails with a
/// unique-constraint error.
pub async fn rename(pool: &SqlitePool, id: i64, name: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("UPDATE portfolios SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

/// Archives a portfolio at `at`, or restores it with `None`. Returns `false` when
/// it is already in that state; an archived portfolio keeps its first archive time.
pub async fn set_archived(
    pool: &SqlitePool,
    id: i64,
    at: Option<Timestamp>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE portfolios SET archived_at = ?1
         WHERE id = ?2 AND (archived_at IS NULL) = (?1 IS NOT NULL)",
    )
    .bind(at.map(time::to_text))
    .bind(id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Shares a portfolio with `user_id` as `role`, or changes the role of an
/// existing share.
pub async fn share(
    pool: &SqlitePool,
    portfolio_id: i64,
    user_id: i64,
    role: ShareRole,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO portfolio_shares (portfolio_id, user_id, role, created_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (portfolio_id, user_id) DO UPDATE SET role = excluded.role",
    )
    .bind(portfolio_id)
    .bind(user_id)
    .bind(Role::from(role).as_str())
    .bind(time::to_text(now))
    .execute(pool)
    .await?;
    Ok(())
}

/// The users a portfolio is shared with, by username.
pub async fn shares(pool: &SqlitePool, portfolio_id: i64) -> Result<Vec<Share>, sqlx::Error> {
    let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as(
        "SELECT u.id, u.username, u.display_name, s.role, s.created_at
         FROM portfolio_shares s JOIN users u ON u.id = s.user_id
         WHERE s.portfolio_id = ?
         ORDER BY u.username COLLATE NOCASE",
    )
    .bind(portfolio_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(user_id, username, display_name, role, created_at)| {
            let role = Role::parse(&role).ok_or_else(|| {
                sqlx::Error::Decode(format!("unknown share role `{role}`").into())
            })?;
            Ok(Share {
                user_id,
                username,
                display_name,
                role,
                created_at,
            })
        })
        .collect()
}

/// Stops sharing a portfolio with `user_id`.
pub async fn unshare(
    pool: &SqlitePool,
    portfolio_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM portfolio_shares WHERE portfolio_id = ? AND user_id = ?")
        .bind(portfolio_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}
