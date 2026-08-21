//! Sessions, transcripts, settings, models — and the search over them.
//!
//! Slice 2 of `docs/core-extraction.md`, and the one the plan originally wanted
//! first, for a reason that still holds: this is the part that was expensive to
//! get right and cheapest to get subtly wrong. The trigram tokenizer and the
//! three-character routing rule below took a measurement campaign to find and
//! are invisible to anyone reading the schema. A second implementation in Swift
//! would not be a duplicate so much as a slow divergence — search would simply
//! be worse on one platform, with nothing in either codebase to point at.
//!
//! Both shells open the *same file*. That is deliberate: it is one product with
//! one history, and a native app with its own database would have forked the
//! user's transcripts on the day it was installed.
//!
//! ## Dates
//!
//! Timestamps cross the boundary as RFC 3339 strings, because that is what is in
//! the column. Converting to a platform date type here would mean choosing a
//! calendar and a timezone in the layer that has the least idea which are right,
//! and would make the value that came out different from the value stored.

use std::sync::Mutex;

use chrono::Local;
use rusqlite::{params, types::Value, Connection, OptionalExtension};
use uuid::Uuid;

/// The one thing here that is genuinely a failure rather than an answer: the
/// database is unreachable, or a statement is wrong. Both are bugs or broken
/// installs, not situations the user can resolve by rephrasing.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StorageError {
    #[error("{detail}")]
    Unavailable { detail: String },
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        StorageError::Unavailable {
            detail: err.to_string(),
        }
    }
}

type Result<T> = std::result::Result<T, StorageError>;

// MARK: - Records

#[derive(Debug, Clone, uniffi::Record)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    /// `YYYY-MM-DD`. That format sorts lexicographically, so a plain string
    /// comparison is a date comparison — which is why the filters below can
    /// bind it directly.
    pub date_key: String,
    pub model: String,
    pub language: String,
    pub runtime: String,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TranscriptRecord {
    pub id: String,
    pub session_id: String,
    pub text: String,
    pub status: String,
    pub source: String,
    pub created_at: String,
    pub duration_ms: Option<i64>,
    pub model: String,
    pub language: String,
    pub formatted_text: Option<String>,
    pub formatted_preset: Option<String>,
    pub formatted_at: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ModelRecord {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub source: String,
    pub repo_id: String,
    pub local_path: String,
    pub status: String,
    pub size_bytes: Option<i64>,
    pub installed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, uniffi::Enum)]
pub enum ArchiveScope {
    /// Everything the user has not put away. The default view.
    #[default]
    Active,
    Archived,
    All,
}

impl ArchiveScope {
    /// SQL predicate over `sessions`, aliased as `s`.
    fn predicate(self) -> Option<&'static str> {
        match self {
            ArchiveScope::Active => Some("s.archived_at IS NULL"),
            ArchiveScope::Archived => Some("s.archived_at IS NOT NULL"),
            ArchiveScope::All => None,
        }
    }
}

/// Narrowing shared by the session list and by search.
///
/// The same value narrows both on purpose: a hit belonging to a session the
/// sidebar is hiding is a hit the user cannot then open.
#[derive(Debug, Clone, Default, uniffi::Record)]
pub struct SessionFilter {
    pub language: Option<String>,
    pub model: Option<String>,
    /// Inclusive `YYYY-MM-DD` bounds.
    pub from: Option<String>,
    pub to: Option<String>,
    pub archived: ArchiveScope,
}

impl SessionFilter {
    /// Non-empty, trimmed value or `None` — an empty select box is not a filter.
    fn cleaned(value: &Option<String>) -> Option<&str> {
        value.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }

    /// Appends `AND …` clauses for whichever fields are set, pushing their bound
    /// values onto `binds` in the same order.
    ///
    /// `language_col` and `model_col` are qualified column names so search can
    /// filter on the *transcript's* own language while the session list filters
    /// on the session's. The row on screen in a search result is a transcript,
    /// so it should be judged by what actually produced it rather than by what
    /// the session was set to when it was opened.
    fn push_sql(&self, language_col: &str, model_col: &str, sql: &mut String, binds: &mut Vec<Value>) {
        if let Some(language) = Self::cleaned(&self.language) {
            sql.push_str(&format!(" AND {language_col} = ?"));
            binds.push(Value::Text(language.to_string()));
        }
        if let Some(model) = Self::cleaned(&self.model) {
            sql.push_str(&format!(" AND {model_col} = ?"));
            binds.push(Value::Text(model.to_string()));
        }
        if let Some(from) = Self::cleaned(&self.from) {
            sql.push_str(" AND s.date_key >= ?");
            binds.push(Value::Text(from.to_string()));
        }
        if let Some(to) = Self::cleaned(&self.to) {
            sql.push_str(" AND s.date_key <= ?");
            binds.push(Value::Text(to.to_string()));
        }
        if let Some(predicate) = self.archived.predicate() {
            sql.push_str(" AND ");
            sql.push_str(predicate);
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FilterOptions {
    pub languages: Vec<String>,
    pub models: Vec<String>,
    pub earliest_date: Option<String>,
    pub latest_date: Option<String>,
    pub archived_count: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchHit {
    pub transcript_id: String,
    pub session_id: String,
    pub session_title: String,
    pub date_key: String,
    pub created_at: String,
    pub language: String,
    pub model: String,
    pub archived: bool,
    /// A window of the transcript around the first match, elided with `…`.
    pub snippet: String,
}

/// Which route answered the query.
///
/// `Substring` means a term was shorter than the three characters the trigram
/// index needs, so the query was answered by scanning. The distinction matters
/// for explaining an empty result, not for how hits are displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchMode {
    Empty,
    Fts,
    Substring,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResults {
    /// The terms actually searched for, for highlighting inside each snippet.
    pub terms: Vec<String>,
    pub hits: Vec<SearchHit>,
    /// More matched than the limit; these are the most recent.
    pub truncated: bool,
    pub mode: SearchMode,
}

// MARK: - The store

/// The database, and everything that reads or writes it.
///
/// A `Mutex<Connection>` rather than a pool: this is one desktop user, the
/// queries are single-digit milliseconds, and a pool would add a failure mode
/// (exhaustion) in exchange for concurrency nobody here has.
#[derive(uniffi::Object)]
pub struct Store {
    conn: Mutex<Connection>,
}

#[uniffi::export]
impl Store {
    /// Open, creating and migrating the schema if needed.
    ///
    /// Safe to call against a database the Tauri build has already made: the
    /// schema statements are all `IF NOT EXISTS`, and the migrations check
    /// `pragma_table_info` before adding a column.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<std::sync::Arc<Self>> {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).map_err(|err| StorageError::Unavailable {
                detail: format!("Could not create {}: {err}", parent.display()),
            })?;
        }
        let conn = Connection::open(&path)?;
        // Write-ahead logging, so a read while the other shell is writing does
        // not block. Both builds open the same file and either may be running.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        apply_schema(&conn)?;
        Ok(std::sync::Arc::new(Self {
            conn: Mutex::new(conn),
        }))
    }

    // MARK: Settings

    pub fn settings(&self) -> Result<Vec<SettingPair>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SettingPair {
                    key: row.get(0)?,
                    value: row.get(1)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Blank counts as absent: a setting written as an empty string is a
    /// setting the user cleared, not one they set to nothing.
    pub fn setting(&self, key: String) -> Result<Option<String>> {
        let conn = self.lock()?;
        let value: Option<String> = conn
            .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(value.filter(|value| !value.trim().is_empty()))
    }

    pub fn set_setting(&self, key: String, value: String) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // MARK: Models

    pub fn models(&self) -> Result<Vec<ModelRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, backend, source, repo_id, local_path, status,
                    size_bytes, installed_at, last_error
             FROM models ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], map_model)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Insert the built-in catalogue if it is not there.
    ///
    /// `INSERT OR IGNORE`, so a row the user already has — with its download
    /// status and installed size — is never overwritten by the defaults.
    pub fn seed_models(&self, models: Vec<ModelRecord>) -> Result<()> {
        let conn = self.lock()?;
        for model in models {
            conn.execute(
                "INSERT OR IGNORE INTO models
                 (id, name, backend, source, repo_id, local_path, status, size_bytes, installed_at, last_error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    model.id, model.name, model.backend, model.source, model.repo_id,
                    model.local_path, model.status, model.size_bytes, model.installed_at,
                    model.last_error
                ],
            )?;
        }
        Ok(())
    }

    // MARK: Sessions

    pub fn sessions(&self, limit: i64, filter: SessionFilter) -> Result<Vec<SessionRecord>> {
        let conn = self.lock()?;
        // `WHERE 1 = 1` so every clause below can be appended uniformly.
        let mut sql = format!("SELECT {SESSION_COLUMNS} FROM sessions s WHERE 1 = 1");
        let mut binds: Vec<Value> = Vec::new();
        filter.push_sql("s.language", "s.model", &mut sql, &mut binds);
        sql.push_str(" ORDER BY s.started_at DESC LIMIT ?");
        binds.push(Value::Integer(limit.max(1)));

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(binds), map_session)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn create_session(
        &self,
        title: Option<String>,
        model: String,
        language: String,
        runtime: String,
    ) -> Result<SessionRecord> {
        let now = Local::now();
        let session = SessionRecord {
            id: Uuid::new_v4().to_string(),
            title: title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| format!("Session {}", now.format("%H:%M"))),
            started_at: now.to_rfc3339(),
            ended_at: None,
            date_key: now.format("%Y-%m-%d").to_string(),
            model,
            language,
            runtime,
            archived_at: None,
        };

        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO sessions
             (id, title, started_at, ended_at, date_key, model, language, runtime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session.id, session.title, session.started_at, session.ended_at,
                session.date_key, session.model, session.language, session.runtime
            ],
        )?;
        Ok(session)
    }

    /// Move a session in or out of the archive.
    ///
    /// Nothing is deleted and nothing is copied: only `archived_at` changes, so
    /// the transcripts, the FTS index and every id stay exactly as they were.
    pub fn set_archived(&self, session_id: String, archived: bool) -> Result<Option<SessionRecord>> {
        let conn = self.lock()?;
        let stamp = archived.then(|| Local::now().to_rfc3339());
        conn.execute(
            "UPDATE sessions SET archived_at = ?1 WHERE id = ?2",
            params![stamp, session_id],
        )?;
        let row = conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM sessions s WHERE s.id = ?1"),
                params![session_id],
                map_session,
            )
            .optional()?;
        Ok(row)
    }

    /// Drop sessions that never captured anything.
    ///
    /// Opening the app creates a session the moment you hold the key, and
    /// changing your mind leaves an empty one behind. Left alone the sidebar
    /// fills with rows that contain nothing.
    pub fn prune_empty_sessions(&self) -> Result<u32> {
        let conn = self.lock()?;
        let removed = conn.execute(
            "DELETE FROM sessions
             WHERE id NOT IN (SELECT DISTINCT session_id FROM transcripts)",
            [],
        )?;
        Ok(removed as u32)
    }

    pub fn filter_options(&self) -> Result<FilterOptions> {
        let conn = self.lock()?;
        let collect = |sql: &str| -> Result<Vec<String>> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        };

        let languages =
            collect("SELECT DISTINCT language FROM sessions WHERE language <> '' ORDER BY language")?;
        let models = collect("SELECT DISTINCT model FROM sessions WHERE model <> '' ORDER BY model")?;
        let (earliest_date, latest_date) = conn.query_row(
            "SELECT MIN(date_key), MAX(date_key) FROM sessions",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )?;
        let archived_count = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE archived_at IS NOT NULL",
            [],
            |row| row.get(0),
        )?;

        Ok(FilterOptions {
            languages,
            models,
            earliest_date,
            latest_date,
            archived_count,
        })
    }

    // MARK: Transcripts

    pub fn transcripts(&self, session_id: String) -> Result<Vec<TranscriptRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, text, status, source, created_at, duration_ms, model, language,
                    formatted_text, formatted_preset, formatted_at
             FROM transcripts WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], map_transcript)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn append_transcript(
        &self,
        session_id: String,
        text: String,
        model: String,
        language: String,
        duration_ms: Option<i64>,
    ) -> Result<TranscriptRecord> {
        let transcript = TranscriptRecord {
            id: Uuid::new_v4().to_string(),
            session_id,
            text,
            status: "final".to_string(),
            source: "local".to_string(),
            created_at: Local::now().to_rfc3339(),
            duration_ms,
            model,
            language,
            // Formatting happens later, asynchronously, if at all.
            formatted_text: None,
            formatted_preset: None,
            formatted_at: None,
        };

        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO transcripts
             (id, session_id, text, status, source, created_at, duration_ms, model, language)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                transcript.id, transcript.session_id, transcript.text, transcript.status,
                transcript.source, transcript.created_at, transcript.duration_ms,
                transcript.model, transcript.language
            ],
        )?;

        // Name the session after what was actually said.
        //
        // A sidebar of "Voice 15:41 / Voice 14:13 / Voice 13:02" carries no
        // information at all — every row looks identical and finding anything
        // means opening them one by one. The first thing spoken is almost
        // always the best short label, and it costs nothing to derive.
        let title = summarize_for_title(&transcript.text);
        if !title.is_empty() {
            // Only while the session still has its generated name, so a title
            // the user set by hand is never overwritten.
            conn.execute(
                "UPDATE sessions SET title = ?1
                   WHERE id = ?2
                     AND (title LIKE 'Voice %' OR title LIKE 'Session %'
                          OR title = 'Voice note' OR title = '')",
                params![title, transcript.session_id],
            )?;
        }

        Ok(transcript)
    }

    /// Store the Markdown a formatting pass produced, beside the spoken text.
    ///
    /// The raw `text` is never touched: the user dictated it, and the formatted
    /// version is a derived view they can discard or regenerate.
    pub fn save_formatted(
        &self,
        transcript_id: String,
        markdown: String,
        preset: String,
    ) -> Result<Option<TranscriptRecord>> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE transcripts
             SET formatted_text = ?2, formatted_preset = ?3, formatted_at = ?4
             WHERE id = ?1",
            params![transcript_id, markdown, preset, Local::now().to_rfc3339()],
        )?;
        let row = conn
            .query_row(
                "SELECT id, session_id, text, status, source, created_at, duration_ms, model,
                        language, formatted_text, formatted_preset, formatted_at
                 FROM transcripts WHERE id = ?1",
                params![transcript_id],
                map_transcript,
            )
            .optional()?;
        Ok(row)
    }

    // MARK: Search

    /// Full-text search across every transcript, newest match first.
    ///
    /// Fast enough to run on every keystroke — it is a local index — which is
    /// why there is no notion of a search being "started".
    pub fn search(&self, query: String, filter: SessionFilter, limit: i64) -> Result<SearchResults> {
        let terms = search_terms(&query);
        if terms.is_empty() {
            return Ok(SearchResults {
                terms,
                hits: Vec::new(),
                truncated: false,
                mode: SearchMode::Empty,
            });
        }

        // The trigram index answers anything three characters or longer. A
        // shorter term — 「的」, `UI` — has no trigram to look up, so those
        // queries scan instead. Mixing the two would need an intersection
        // across two indexes for no real gain, so a single short term puts the
        // whole query on the scan.
        let use_fts = terms
            .iter()
            .all(|term| term.chars().count() >= MIN_TRIGRAM_CHARS);

        let mut binds: Vec<Value> = Vec::new();
        let mut sql = String::from(
            "SELECT t.id, t.session_id, t.text, t.created_at, t.language, t.model,
                    s.title, s.date_key, s.archived_at
             FROM transcripts t
             JOIN sessions s ON s.id = t.session_id
             WHERE ",
        );

        if use_fts {
            sql.push_str("t.rowid IN (SELECT rowid FROM transcripts_fts WHERE transcripts_fts MATCH ?)");
            binds.push(Value::Text(fts_match_expression(&terms)));
        } else {
            // All terms must appear, matching the AND semantics of the FTS path.
            for (index, term) in terms.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                sql.push_str("t.text LIKE ? ESCAPE '\\'");
                binds.push(Value::Text(like_pattern(term)));
            }
        }

        filter.push_sql("t.language", "t.model", &mut sql, &mut binds);

        // One row over the limit, purely to tell "exactly full" from "there is
        // more".
        sql.push_str(" ORDER BY t.created_at DESC LIMIT ?");
        binds.push(Value::Integer(limit.max(1) + 1));

        let conn = self.lock()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut hits = stmt
            .query_map(rusqlite::params_from_iter(binds), |row| {
                let text: String = row.get(2)?;
                let archived: Option<String> = row.get(8)?;
                Ok(SearchHit {
                    transcript_id: row.get(0)?,
                    session_id: row.get(1)?,
                    snippet: build_snippet(&text, &terms, SNIPPET_CHARS),
                    created_at: row.get(3)?,
                    language: row.get(4)?,
                    model: row.get(5)?,
                    session_title: row.get(6)?,
                    date_key: row.get(7)?,
                    archived: archived.is_some(),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let truncated = hits.len() as i64 > limit.max(1);
        hits.truncate(limit.max(1) as usize);

        Ok(SearchResults {
            terms,
            hits,
            truncated,
            mode: if use_fts {
                SearchMode::Fts
            } else {
                SearchMode::Substring
            },
        })
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SettingPair {
    pub key: String,
    pub value: String,
}

impl Store {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| StorageError::Unavailable {
            detail: "The database lock was poisoned by an earlier failure. Restart the app."
                .to_string(),
        })
    }
}

const SESSION_COLUMNS: &str =
    "s.id, s.title, s.started_at, s.ended_at, s.date_key, s.model, s.language, s.runtime, s.archived_at";

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        date_key: row.get(4)?,
        model: row.get(5)?,
        language: row.get(6)?,
        runtime: row.get(7)?,
        archived_at: row.get(8)?,
    })
}

fn map_transcript(row: &rusqlite::Row<'_>) -> rusqlite::Result<TranscriptRecord> {
    Ok(TranscriptRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        text: row.get(2)?,
        status: row.get(3)?,
        source: row.get(4)?,
        created_at: row.get(5)?,
        duration_ms: row.get(6)?,
        model: row.get(7)?,
        language: row.get(8)?,
        formatted_text: row.get(9)?,
        formatted_preset: row.get(10)?,
        formatted_at: row.get(11)?,
    })
}

fn map_model(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelRecord> {
    Ok(ModelRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        backend: row.get(2)?,
        source: row.get(3)?,
        repo_id: row.get(4)?,
        local_path: row.get(5)?,
        status: row.get(6)?,
        size_bytes: row.get(7)?,
        installed_at: row.get(8)?,
        last_error: row.get(9)?,
    })
}

// MARK: - Search helpers

/// Splits a raw query box into search terms.
///
/// Whitespace-separated, because that is what every search box in the world
/// does. Quoted phrases are deliberately not supported: with a trigram index
/// every term is already a literal substring, so `"液态 玻璃"` and `液态 玻璃`
/// would only differ in whether the space itself has to match.
fn search_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

/// Wraps each term as an FTS5 string literal and ANDs them together.
///
/// Every term becomes a `"…"` phrase so that punctuation and CJK inside it are
/// matched literally rather than parsed as FTS5 operator syntax; an embedded
/// double quote is escaped by doubling, as SQL requires.
fn fts_match_expression(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes a term for use inside `LIKE '%' || ? || '%' ESCAPE '\'`.
fn like_pattern(term: &str) -> String {
    let mut escaped = String::with_capacity(term.len() + 2);
    escaped.push('%');
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped.push('%');
    escaped
}

/// The trigram tokenizer cannot answer a query shorter than one trigram.
const MIN_TRIGRAM_CHARS: usize = 3;
const SNIPPET_CHARS: usize = 110;

/// A window of `text` around the first term that occurs in it.
///
/// Counted in `char`s, never bytes: a byte window would give a Chinese snippet a
/// third the content of an English one and could split a character in half.
fn build_snippet(text: &str, terms: &[String], window: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= window {
        return text.trim().to_string();
    }

    // Case-insensitive search on a lowered copy, indexed by char position so the
    // offset maps straight back onto `chars`.
    let lowered: Vec<char> = chars.iter().flat_map(|ch| ch.to_lowercase()).collect();
    let hit = terms.iter().find_map(|term| {
        let needle: Vec<char> = term.chars().flat_map(|ch| ch.to_lowercase()).collect();
        if needle.is_empty() || needle.len() > lowered.len() {
            return None;
        }
        (0..=lowered.len() - needle.len())
            .find(|&start| lowered[start..start + needle.len()] == needle[..])
    });

    // `to_lowercase` can change a string's length (ß, İ), which would shift the
    // offset. Clamping keeps the window inside the text either way; the worst
    // case is a snippet centred a character or two off.
    let hit = hit.unwrap_or(0).min(chars.len().saturating_sub(1));
    let lead = window / 3;
    let start = hit.saturating_sub(lead);
    let end = (start + window).min(chars.len());
    let start = end.saturating_sub(window);

    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(&chars[start..end]);
    if end < chars.len() {
        snippet.push('…');
    }
    snippet
}

/// First clause of a transcript, trimmed to something that fits a sidebar.
///
/// Counts characters rather than bytes: Chinese is three bytes per character in
/// UTF-8, so a byte limit would cut CJK titles to a third the length of English
/// ones, and could slice a character in half.
fn summarize_for_title(text: &str) -> String {
    const MAX_CHARS: usize = 28;
    let cleaned = text.trim();

    // Prefer a natural break, in either script's punctuation.
    let first = cleaned
        .split(['。', '！', '？', '\n', '.', '!', '?'])
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(cleaned);

    let mut chars = first.chars();
    let head: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

// MARK: - Schema

fn apply_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS models (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            backend TEXT NOT NULL,
            source TEXT NOT NULL,
            repo_id TEXT NOT NULL,
            local_path TEXT NOT NULL,
            status TEXT NOT NULL,
            size_bytes INTEGER,
            installed_at TEXT,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            date_key TEXT NOT NULL,
            model TEXT NOT NULL,
            language TEXT NOT NULL,
            runtime TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS transcripts (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            text TEXT NOT NULL,
            status TEXT NOT NULL,
            source TEXT NOT NULL,
            created_at TEXT NOT NULL,
            duration_ms INTEGER,
            model TEXT NOT NULL,
            language TEXT NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS segments (
            id TEXT PRIMARY KEY,
            transcript_id TEXT NOT NULL,
            text TEXT NOT NULL,
            start_ms INTEGER,
            end_ms INTEGER,
            confidence REAL,
            FOREIGN KEY(transcript_id) REFERENCES transcripts(id) ON DELETE CASCADE
        );

        -- tokenize='trigram', not the default unicode61.
        --
        -- unicode61 splits on spaces and punctuation, which is meaningless for
        -- Chinese: 「今天所做的这一个渲染」 is one enormous token, so searching
        -- 「渲染」 matches nothing at all. The trigram tokenizer indexes every
        -- overlapping run of three characters instead, which makes MATCH a true
        -- substring search and works for CJK, Latin and mixed text alike. The
        -- cost is a larger index and a three-character floor on queries —
        -- shorter ones fall back to LIKE in `search`.
        CREATE VIRTUAL TABLE IF NOT EXISTS transcripts_fts USING fts5(
            text,
            content='transcripts',
            content_rowid='rowid',
            tokenize='trigram'
        );

        CREATE TRIGGER IF NOT EXISTS transcripts_ai AFTER INSERT ON transcripts BEGIN
            INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS transcripts_ad AFTER DELETE ON transcripts BEGIN
            INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
        END;

        CREATE TRIGGER IF NOT EXISTS transcripts_au AFTER UPDATE ON transcripts BEGIN
            INSERT INTO transcripts_fts(transcripts_fts, rowid, text)
            VALUES ('delete', old.rowid, old.text);
            INSERT INTO transcripts_fts(rowid, text) VALUES (new.rowid, new.text);
        END;
        "#,
    )?;

    // Two-layer transcripts: raw ASR text plus an optional LLM-formatted
    // version. Added after the initial schema, so applied as a migration.
    for (column, decl) in [
        ("formatted_text", "TEXT"),
        ("formatted_preset", "TEXT"),
        ("formatted_at", "TEXT"),
    ] {
        let exists: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('transcripts') WHERE name = ?1")?
            .exists(params![column])?;
        if !exists {
            conn.execute(&format!("ALTER TABLE transcripts ADD COLUMN {column} {decl}"), [])?;
        }
    }

    // Archiving. A timestamp rather than a boolean, because "when did I put this
    // away" is worth keeping and costs nothing over a flag.
    let archived_exists: bool = conn
        .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = ?1")?
        .exists(params!["archived_at"])?;
    if !archived_exists {
        conn.execute("ALTER TABLE sessions ADD COLUMN archived_at TEXT", [])?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_sessions_archived_started
             ON sessions(archived_at, started_at DESC);
         CREATE INDEX IF NOT EXISTS idx_transcripts_session
             ON transcripts(session_id, created_at);",
    )?;

    // Re-tokenize an index built before trigram.
    //
    // `CREATE VIRTUAL TABLE IF NOT EXISTS` above is a no-op on databases that
    // already have the unicode61 index, and those cannot match Chinese at all.
    // The FTS content lives entirely in `transcripts`, so dropping and
    // rebuilding loses nothing — it is a derived index, not data.
    let fts_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'transcripts_fts'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if fts_sql.is_some_and(|sql| !sql.contains("trigram")) {
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS transcripts_fts;
            CREATE VIRTUAL TABLE transcripts_fts USING fts5(
                text,
                content='transcripts',
                content_rowid='rowid',
                tokenize='trigram'
            );
            INSERT INTO transcripts_fts(transcripts_fts) VALUES('rebuild');
            "#,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> std::sync::Arc<Store> {
        // A file, not `:memory:`. The FTS5 external-content table and its
        // triggers are exactly the part worth testing, and testing them against
        // a different storage mode would be testing something else.
        let path = std::env::temp_dir().join(format!("koushu-test-{}.sqlite3", Uuid::new_v4()));
        Store::open(path.to_string_lossy().to_string()).unwrap()
    }

    fn seeded() -> (std::sync::Arc<Store>, SessionRecord) {
        let store = store();
        let session = store
            .create_session(Some("Test".into()), "m".into(), "中文".into(), "r".into())
            .unwrap();
        for text in [
            "这是一段用来测试转写的中文语音，说完就结束。",
            "The non-activating panel is the load-bearing part of this app.",
        ] {
            store
                .append_transcript(session.id.clone(), text.into(), "m".into(), "中文".into(), None)
                .unwrap();
        }
        (store, session)
    }

    #[test]
    fn chinese_substrings_are_findable_at_all() {
        // The whole reason for tokenize='trigram'. With unicode61 the sentence
        // above is one token and this returns nothing.
        let (store, _) = seeded();
        let results = store.search("转写".into(), SessionFilter::default(), 20).unwrap();
        assert_eq!(results.mode, SearchMode::Substring, "two chars is under the floor");
        assert_eq!(results.hits.len(), 1);

        let longer = store.search("中文语音".into(), SessionFilter::default(), 20).unwrap();
        assert_eq!(longer.mode, SearchMode::Fts);
        assert_eq!(longer.hits.len(), 1);
    }

    #[test]
    fn every_term_must_appear_on_both_routes() {
        let (store, _) = seeded();
        // Both terms ≥3 characters, so this takes the index path. One matches
        // and one does not, so the AND fails. (`语音` would *not* do here: two
        // characters is under the trigram floor and would route to the scan,
        // which is the other half of this test.)
        let fts = store
            .search("中文语音 panel".into(), SessionFilter::default(), 20)
            .unwrap();
        assert_eq!(fts.mode, SearchMode::Fts);
        assert!(fts.hits.is_empty(), "AND semantics on the index path");

        // A short term forces the scan; the same AND must hold there.
        let scan = store.search("的 panel".into(), SessionFilter::default(), 20).unwrap();
        assert_eq!(scan.mode, SearchMode::Substring);
        assert!(scan.hits.is_empty(), "AND semantics on the scan path");
    }

    #[test]
    fn a_term_containing_fts_syntax_is_matched_literally() {
        let store = store();
        let session = store
            .create_session(None, "m".into(), "en".into(), "r".into())
            .unwrap();
        store
            .append_transcript(
                session.id.clone(),
                "the value is NOT NULL and \"quoted\" here".into(),
                "m".into(),
                "en".into(),
                None,
            )
            .unwrap();
        // `NOT` is an FTS5 operator. Unquoted it would be parsed as one and
        // either error or mean something else entirely.
        let results = store.search("NOT NULL".into(), SessionFilter::default(), 20).unwrap();
        assert_eq!(results.hits.len(), 1);
    }

    #[test]
    fn archiving_hides_from_both_the_list_and_the_search() {
        let (store, session) = seeded();
        store.set_archived(session.id.clone(), true).unwrap();

        let active = store.sessions(50, SessionFilter::default()).unwrap();
        assert!(active.is_empty());

        let hidden = store.search("语音".into(), SessionFilter::default(), 20).unwrap();
        assert!(hidden.hits.is_empty(), "a hit you cannot open is not a hit");

        let everything = store
            .search(
                "语音".into(),
                SessionFilter {
                    archived: ArchiveScope::All,
                    ..Default::default()
                },
                20,
            )
            .unwrap();
        assert_eq!(everything.hits.len(), 1);
    }

    #[test]
    fn a_session_takes_its_name_from_the_first_thing_said() {
        let store = store();
        let session = store
            .create_session(None, "m".into(), "中文".into(), "r".into())
            .unwrap();
        assert!(session.title.starts_with("Session "));

        store
            .append_transcript(
                session.id.clone(),
                "把这条语音条改写成原生的。然后再说别的。".into(),
                "m".into(),
                "中文".into(),
                None,
            )
            .unwrap();
        let renamed = &store.sessions(10, SessionFilter::default()).unwrap()[0];
        assert_eq!(renamed.title, "把这条语音条改写成原生的");
    }

    #[test]
    fn a_title_the_user_chose_is_never_overwritten() {
        let store = store();
        let session = store
            .create_session(Some("我的会议".into()), "m".into(), "中文".into(), "r".into())
            .unwrap();
        store
            .append_transcript(session.id.clone(), "随便说点什么。".into(), "m".into(), "中文".into(), None)
            .unwrap();
        assert_eq!(store.sessions(10, SessionFilter::default()).unwrap()[0].title, "我的会议");
    }

    #[test]
    fn formatting_never_touches_what_was_said() {
        let (store, session) = seeded();
        let original = store.transcripts(session.id.clone()).unwrap()[0].clone();
        store
            .save_formatted(original.id.clone(), "> typeset".into(), "typeset".into())
            .unwrap();
        let after = &store.transcripts(session.id).unwrap()[0];
        assert_eq!(after.text, original.text);
        assert_eq!(after.formatted_text.as_deref(), Some("> typeset"));
    }

    #[test]
    fn empty_sessions_are_pruned_and_full_ones_are_not() {
        let (store, session) = seeded();
        store.create_session(None, "m".into(), "en".into(), "r".into()).unwrap();
        assert_eq!(store.prune_empty_sessions().unwrap(), 1);
        let left = store.sessions(50, SessionFilter::default()).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, session.id);
    }

    #[test]
    fn seeding_models_never_clobbers_an_installed_one() {
        let store = store();
        let model = ModelRecord {
            id: "m1".into(),
            name: "M".into(),
            backend: "b".into(),
            source: "hf".into(),
            repo_id: "r".into(),
            local_path: "/tmp".into(),
            status: "available".into(),
            size_bytes: None,
            installed_at: None,
            last_error: None,
        };
        store.seed_models(vec![model.clone()]).unwrap();
        store
            .seed_models(vec![ModelRecord {
                status: "installed".into(),
                ..model.clone()
            }])
            .unwrap();
        // The second seed must not have downgraded it, and must not have
        // upgraded it either — the row on disk wins.
        assert_eq!(store.models().unwrap()[0].status, "available");
    }

    #[test]
    fn blank_settings_read_as_absent() {
        let store = store();
        store.set_setting("k".into(), "  ".into()).unwrap();
        assert_eq!(store.setting("k".into()).unwrap(), None);
        store.set_setting("k".into(), "v".into()).unwrap();
        assert_eq!(store.setting("k".into()).unwrap(), Some("v".into()));
    }

    #[test]
    fn truncation_is_reported_rather_than_silent() {
        let store = store();
        let session = store
            .create_session(None, "m".into(), "en".into(), "r".into())
            .unwrap();
        for _ in 0..5 {
            store
                .append_transcript(session.id.clone(), "needle here".into(), "m".into(), "en".into(), None)
                .unwrap();
        }
        let results = store.search("needle".into(), SessionFilter::default(), 3).unwrap();
        assert_eq!(results.hits.len(), 3);
        assert!(results.truncated);
    }
}
