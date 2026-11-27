//! Reading a CSV against a column mapping: who a row says it belongs to, which
//! instrument it names, and what it would store.

use std::collections::{BTreeMap, HashMap};

use jiff::civil::Date;
use rust_decimal::Decimal;
use saffron_core::{book as engine, instrument::Isin};
use saffron_db::{
    accounts::{self, Account},
    decimal, instruments,
};

use super::{IMPORT_CANDIDATE_BASE, ImportRequest, MAX_CSV_BYTES, MAX_ROWS};
use crate::{
    book::transactions::MAX_NOTE_CHARS,
    error::{ApiError, ApiResult},
    state::AppState,
};

/// Why a row did not parse: the row's own fault, or a server failure.
#[derive(Debug)]
enum Failure {
    /// The row itself is wrong; the text is shown to the client.
    Row(String),
    /// The server could not answer; the request stops.
    Server(ApiError),
}

impl From<String> for Failure {
    fn from(text: String) -> Self {
        Self::Row(text)
    }
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Server(error.into())
    }
}

/// The column positions of one request.
#[derive(Debug, Clone, Copy)]
struct Indices {
    account: usize,
    kind: usize,
    date: usize,
    instrument: Option<usize>,
    quantity: Option<usize>,
    price: Option<usize>,
    amount: Option<usize>,
    fee: Option<usize>,
    currency: Option<usize>,
    fx_rate: Option<usize>,
    note: Option<usize>,
}

/// A parsed row ready to store.
#[derive(Debug)]
pub(super) struct Row {
    /// The data row in the file, counting from one.
    pub(super) line: usize,
    /// The account's name, for a closed-account message.
    pub(super) account: String,
    /// Whether the account is closed; it then takes no new row.
    pub(super) closed: bool,
    pub(super) transaction: engine::Transaction,
    pub(super) note: Option<String>,
    pub(super) key: DuplicateKey,
}

/// What makes two rows the same transaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct DuplicateKey {
    account_id: i64,
    kind: String,
    date: String,
    instrument_id: Option<i64>,
    quantity: Option<String>,
    price: Option<String>,
    amount: Option<String>,
    fee: String,
    currency: String,
    fx_rate: Option<String>,
}

impl DuplicateKey {
    pub(super) fn of(transaction: &engine::Transaction) -> Self {
        let text = |value: Option<Decimal>| value.map(decimal::to_text);
        Self {
            account_id: transaction.account_id,
            kind: transaction.kind.as_str().to_owned(),
            date: transaction.date.to_string(),
            instrument_id: transaction.instrument_id,
            quantity: text(transaction.quantity),
            price: text(transaction.price),
            amount: text(transaction.amount),
            fee: decimal::to_text(transaction.fee),
            currency: transaction.currency.clone(),
            fx_rate: text(transaction.fx_rate),
        }
    }
}

/// The accounts and symbols a file's rows resolve against.
struct Resolver {
    accounts: BTreeMap<String, Account>,
    symbols: HashMap<String, i64>,
    isins: HashMap<String, Option<i64>>,
}

impl Resolver {
    async fn load(state: &AppState, portfolio_id: i64) -> ApiResult<Self> {
        let accounts = accounts::list(&state.pool, portfolio_id)
            .await?
            .into_iter()
            .map(|account| (account.name.clone(), account))
            .collect();
        let symbols = instruments::summaries(&state.pool)
            .await?
            .into_iter()
            .filter_map(|summary| {
                summary
                    .symbol
                    .map(|symbol| (symbol.to_ascii_uppercase(), summary.id))
            })
            .collect();
        Ok(Self {
            accounts,
            symbols,
            isins: HashMap::new(),
        })
    }

    /// A closed account still resolves: its stored rows count as duplicates, and
    /// only a new row for it is refused later.
    fn account(&self, name: &str) -> Result<&Account, String> {
        self.accounts
            .get(name)
            .ok_or_else(|| format!("account `{name}` is not in this portfolio"))
    }

    async fn instrument(&mut self, state: &AppState, text: &str) -> Result<Option<i64>, Failure> {
        if text.is_empty() {
            return Ok(None);
        }
        if let Some(id) = self.symbols.get(&text.to_ascii_uppercase()) {
            return Ok(Some(*id));
        }
        let code = text.to_ascii_uppercase();
        if !self.isins.contains_key(&code) {
            let id = if Isin::parse(&code).is_some() {
                instruments::id_by_isin(&state.pool, &code).await?
            } else {
                None
            };
            self.isins.insert(code.clone(), id);
        }
        match self.isins[&code] {
            Some(id) => Ok(Some(id)),
            None => Err(Failure::Row(format!("instrument `{text}` is not known"))),
        }
    }
}

/// Reads the file against the mapping; a row either parses or names its line and
/// first error.
pub(super) async fn parse(
    state: &AppState,
    portfolio_id: i64,
    request: &ImportRequest,
) -> ApiResult<Vec<Result<Row, (usize, String)>>> {
    if request.csv.len() > MAX_CSV_BYTES {
        return Err(ApiError::bad_request(
            "file_too_large",
            format!("a CSV has at most {MAX_CSV_BYTES} bytes"),
        ));
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(request.csv.as_bytes());
    let headers = reader.headers().map_err(|error| csv_error(&error))?.clone();
    let indices = indices(&headers, &request.columns)?;
    let mut resolver = Resolver::load(state, portfolio_id).await?;
    let mut outcomes = Vec::new();
    for (index, record) in reader.records().enumerate() {
        if index == MAX_ROWS {
            return Err(ApiError::bad_request(
                "too_many_rows",
                format!("a file has at most {MAX_ROWS} rows"),
            ));
        }
        let line = index + 1;
        let outcome = match record {
            Ok(record) => match parse_row(state, &mut resolver, &record, indices, line).await {
                Ok(row) => Ok(row),
                Err(Failure::Row(error)) => Err((line, error)),
                Err(Failure::Server(error)) => return Err(error),
            },
            Err(error) => Err((line, format!("the file cannot be read: {error}"))),
        };
        outcomes.push(outcome);
    }
    if outcomes.is_empty() {
        return Err(ApiError::bad_request(
            "empty_file",
            "the file has no data rows",
        ));
    }
    Ok(outcomes)
}

async fn parse_row(
    state: &AppState,
    resolver: &mut Resolver,
    record: &csv::StringRecord,
    indices: Indices,
    line: usize,
) -> Result<Row, Failure> {
    let field = |index: Option<usize>| {
        index
            .and_then(|index| record.get(index))
            .unwrap_or("")
            .trim()
    };
    let account = resolver.account(field(Some(indices.account)))?;
    let (account_id, account_name) = (account.id, account.name.clone());
    let closed = account.closed_at.is_some();
    let account_currency = account.currency.clone();
    let kind_text = field(Some(indices.kind));
    let kind = engine::Kind::parse(kind_text)
        .ok_or_else(|| format!("`{kind_text}` is not a transaction kind"))?;
    let date = parse_date(field(Some(indices.date)))?;
    let instrument_id = resolver
        .instrument(state, field(indices.instrument))
        .await?;
    let quantity = decimal_of("quantity", field(indices.quantity))?;
    let price = decimal_of("price", field(indices.price))?;
    let amount = decimal_of("amount", field(indices.amount))?;
    let fee = decimal_of("fee", field(indices.fee))?.unwrap_or(Decimal::ZERO);
    let currency = currency_of(field(indices.currency), &account_currency)?;
    let fx_rate = decimal_of("fx_rate", field(indices.fx_rate))?;
    let note = note_of(field(indices.note))?;
    let transaction = engine::Transaction {
        id: IMPORT_CANDIDATE_BASE,
        account_id,
        instrument_id,
        kind,
        date,
        quantity,
        price,
        amount,
        fee,
        currency: currency.clone(),
        fx_rate,
    };
    let key = DuplicateKey::of(&transaction);
    Ok(Row {
        line,
        account: account_name,
        closed,
        transaction,
        note,
        key,
    })
}

/// The data row of the imported transaction the engine named.
pub(super) fn line_of(rows: &[Row], transaction: i64) -> usize {
    usize::try_from(transaction - IMPORT_CANDIDATE_BASE)
        .ok()
        .and_then(|index| rows.get(index))
        .map_or(0, |row| row.line)
}

fn indices(headers: &csv::StringRecord, columns: &super::Columns) -> ApiResult<Indices> {
    let column = |wanted: &Option<String>, field: &str| -> ApiResult<Option<usize>> {
        let Some(name) = wanted else {
            return Ok(None);
        };
        let name = name.trim();
        headers
            .iter()
            .position(|header| header.trim() == name)
            .map(Some)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "unknown_column",
                    format!("the file has no column `{name}` for {field}"),
                )
            })
    };
    let required = |wanted: &Option<String>, field: &str| -> ApiResult<usize> {
        column(wanted, field)?.ok_or_else(|| {
            ApiError::bad_request(
                "missing_column",
                format!("the mapping has no {field} column"),
            )
        })
    };
    Ok(Indices {
        account: required(&columns.account, "account")?,
        kind: required(&columns.kind, "kind")?,
        date: required(&columns.date, "date")?,
        instrument: column(&columns.instrument, "instrument")?,
        quantity: column(&columns.quantity, "quantity")?,
        price: column(&columns.price, "price")?,
        amount: column(&columns.amount, "amount")?,
        fee: column(&columns.fee, "fee")?,
        currency: column(&columns.currency, "currency")?,
        fx_rate: column(&columns.fx_rate, "fx_rate")?,
        note: column(&columns.note, "note")?,
    })
}

fn parse_date(text: &str) -> Result<Date, String> {
    text.parse()
        .map_err(|_| format!("date `{text}` is not YYYY-MM-DD"))
}

fn decimal_of(name: &str, text: &str) -> Result<Option<Decimal>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    decimal::from_text(text)
        .map(Some)
        .map_err(|_| format!("{name} `{text}` is not a decimal"))
}

fn currency_of(text: &str, default: &str) -> Result<String, String> {
    if text.is_empty() {
        return Ok(default.to_owned());
    }
    let code = text.to_ascii_uppercase();
    if code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Ok(code);
    }
    Err(format!(
        "currency `{text}` is not a three-letter ISO 4217 code"
    ))
}

fn note_of(text: &str) -> Result<Option<String>, String> {
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().count() > MAX_NOTE_CHARS {
        return Err(format!("a note has at most {MAX_NOTE_CHARS} characters"));
    }
    Ok(Some(text.to_owned()))
}

fn csv_error(error: &csv::Error) -> ApiError {
    ApiError::bad_request("invalid_csv", format!("the file cannot be read: {error}"))
}
