//! SQLite persistence for scheduled tasks.
//!
//! Text only. Captured audio, screenshots and the API key never reach this
//! layer: the key lives in the Windows Credential Manager.
//!
//! The table is migrated in place, so an existing `murmur.sqlite3` from an
//! earlier build keeps its rows while gaining the action, recurrence and status
//! columns the voice scheduler needs.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, Row};

use crate::loadout::{Loadout, SETTINGS_KEY};
use crate::scheduler::{ActionKind, ScheduleKind, ScheduledTask, TaskStatus};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

const COLUMNS: &str = "id, prompt, action_kind, payload, schedule_kind, due_at_ms, \
                       recurrence, recurrence_start_ms, status, created_at_ms, updated_at_ms, fired_at_ms, \
                       fire_count, last_error";

pub struct Database {
    conn: Mutex<Connection>,
}

/// Everything a task needs except its identity and timestamps. Borrowed so the
/// voice path and the commands do not copy strings just to insert them.
pub struct TaskFields<'a> {
    pub prompt: &'a str,
    pub action: ActionKind,
    pub payload: &'a str,
    pub schedule: ScheduleKind,
    pub due_at_ms: i64,
    pub recurrence: Option<&'a str>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).map_err(|err| anyhow!("open sqlite: {err}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )
        .map_err(|err| anyhow!("configure sqlite: {err}"))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow!("database lock poisoned"))
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                prompt        TEXT    NOT NULL,
                action_kind   TEXT    NOT NULL DEFAULT 'remind',
                payload       TEXT    NOT NULL DEFAULT '',
                schedule_kind TEXT    NOT NULL DEFAULT 'one_time',
                due_at_ms     INTEGER NOT NULL,
                recurrence    TEXT,
                recurrence_start_ms INTEGER,
                status        TEXT    NOT NULL DEFAULT 'pending',
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL DEFAULT 0,
                fired_at_ms   INTEGER,
                fire_count    INTEGER NOT NULL DEFAULT 0,
                last_error    TEXT
             );
             CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_due
                ON scheduled_tasks (status, due_at_ms);
             CREATE TABLE IF NOT EXISTS settings (
                key           TEXT PRIMARY KEY,
                value         TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
             );",
        )
        .map_err(|err| anyhow!("run migrations: {err}"))?;

        // `CREATE TABLE IF NOT EXISTS` does not touch a table made by an older
        // build, so add any missing columns explicitly.
        let existing = table_columns(&conn)?;
        let additions: &[(&str, &str)] = &[
            ("action_kind", "action_kind TEXT NOT NULL DEFAULT 'remind'"),
            ("payload", "payload TEXT NOT NULL DEFAULT ''"),
            (
                "schedule_kind",
                "schedule_kind TEXT NOT NULL DEFAULT 'one_time'",
            ),
            ("recurrence", "recurrence TEXT"),
            ("recurrence_start_ms", "recurrence_start_ms INTEGER"),
            ("updated_at_ms", "updated_at_ms INTEGER NOT NULL DEFAULT 0"),
            ("fire_count", "fire_count INTEGER NOT NULL DEFAULT 0"),
            ("last_error", "last_error TEXT"),
        ];

        for (name, definition) in additions {
            if !existing.iter().any(|column| column == name) {
                conn.execute_batch(&format!(
                    "ALTER TABLE scheduled_tasks ADD COLUMN {definition}"
                ))
                .map_err(|err| anyhow!("add column {name}: {err}"))?;
            }
        }

        conn.execute(
            "UPDATE scheduled_tasks SET updated_at_ms = created_at_ms
             WHERE updated_at_ms = 0",
            [],
        )
        .map_err(|err| anyhow!("backfill updated_at_ms: {err}"))?;

        conn.execute(
            "UPDATE scheduled_tasks SET recurrence_start_ms = due_at_ms
             WHERE schedule_kind = 'recurring' AND recurrence_start_ms IS NULL",
            [],
        )
        .map_err(|err| anyhow!("backfill recurrence anchors: {err}"))?;

        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|err| anyhow!("prepare setting lookup: {err}"))?;
        let mut rows = statement
            .query(params![key])
            .map_err(|err| anyhow!("query setting: {err}"))?;

        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?
            .execute(
                "INSERT INTO settings (key, value, updated_at_ms)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at_ms = excluded.updated_at_ms",
                params![key, value, now_ms()],
            )
            .map_err(|err| anyhow!("save setting: {err}"))?;
        Ok(())
    }

    /// Loads and normalizes the Loadout. A missing row means defaults; an
    /// unreadable row falls back to defaults rather than blocking startup.
    pub fn load_loadout(&self) -> Result<Loadout> {
        let raw = self.get_setting(SETTINGS_KEY)?;
        let loadout = raw
            .as_deref()
            .and_then(|json| serde_json::from_str::<Loadout>(json).ok())
            .unwrap_or_default();
        Ok(loadout.normalize())
    }

    pub fn save_loadout(&self, loadout: &Loadout) -> Result<Loadout> {
        let loadout = loadout.clone().normalize();
        let json = serde_json::to_string(&loadout)
            .map_err(|err| anyhow!("serialize the loadout: {err}"))?;
        self.set_setting(SETTINGS_KEY, &json)?;
        Ok(loadout)
    }

    pub fn insert_task(&self, task: &TaskFields<'_>) -> Result<ScheduledTask> {
        if task.prompt.trim().is_empty() {
            return Err(anyhow!("a task needs a prompt"));
        }

        let created_at_ms = now_ms();
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO scheduled_tasks
                (prompt, action_kind, payload, schedule_kind, due_at_ms, recurrence, recurrence_start_ms,
                 status, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                task.prompt.trim(),
                task.action.as_str(),
                task.payload,
                task.schedule.as_str(),
                task.due_at_ms,
                task.recurrence,
                (task.schedule == ScheduleKind::Recurring).then_some(created_at_ms),
                TaskStatus::Pending.as_str(),
                created_at_ms,
            ],
        )
        .map_err(|err| anyhow!("insert task: {err}"))?;

        Ok(ScheduledTask {
            id: conn.last_insert_rowid(),
            prompt: task.prompt.trim().to_string(),
            action: task.action,
            payload: task.payload.to_string(),
            schedule: task.schedule,
            due_at_ms: task.due_at_ms,
            recurrence: task.recurrence.map(str::to_string),
            recurrence_start_ms: (task.schedule == ScheduleKind::Recurring)
                .then_some(created_at_ms),
            status: TaskStatus::Pending,
            created_at_ms,
            updated_at_ms: created_at_ms,
            fired_at_ms: None,
            fire_count: 0,
            last_error: None,
        })
    }

    pub fn get_task(&self, id: i64) -> Result<Option<ScheduledTask>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM scheduled_tasks WHERE id = ?1"
            ))
            .map_err(|err| anyhow!("prepare task lookup: {err}"))?;

        let mut rows = statement
            .query_map(params![id], map_task)
            .map_err(|err| anyhow!("query task: {err}"))?;

        match rows.next() {
            Some(row) => Ok(Some(row.map_err(|err| anyhow!("read task row: {err}"))?)),
            None => Ok(None),
        }
    }

    pub fn list_tasks(&self, limit: i64) -> Result<Vec<ScheduledTask>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM scheduled_tasks
                 ORDER BY CASE WHEN status IN ('pending', 'paused', 'running') THEN 0 ELSE 1 END,
                          due_at_ms ASC
                 LIMIT ?1"
            ))
            .map_err(|err| anyhow!("prepare task list: {err}"))?;

        let rows = statement
            .query_map(params![limit], map_task)
            .map_err(|err| anyhow!("query tasks: {err}"))?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|err| anyhow!("read task row: {err}"))?);
        }
        Ok(tasks)
    }

    pub fn due_tasks(&self, now: i64, limit: i64) -> Result<Vec<ScheduledTask>> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare(&format!(
                "SELECT {COLUMNS} FROM scheduled_tasks
                 WHERE status = 'pending' AND due_at_ms <= ?1
                 ORDER BY due_at_ms ASC
                 LIMIT ?2"
            ))
            .map_err(|err| anyhow!("prepare due-task query: {err}"))?;

        let rows = statement
            .query_map(params![now, limit], map_task)
            .map_err(|err| anyhow!("query due tasks: {err}"))?;

        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(|err| anyhow!("read due-task row: {err}"))?);
        }
        Ok(tasks)
    }

    pub fn update_task(&self, id: i64, task: &TaskFields<'_>) -> Result<Option<ScheduledTask>> {
        if task.prompt.trim().is_empty() {
            return Err(anyhow!("a task needs a prompt"));
        }

        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET
                    prompt = ?2,
                    action_kind = ?3,
                    payload = ?4,
                    schedule_kind = ?5,
                    due_at_ms = ?6,
                    recurrence = ?7,
                    recurrence_start_ms = CASE WHEN ?5 = 'recurring' THEN ?6 ELSE NULL END,
                    status = CASE status WHEN 'paused' THEN 'paused' ELSE 'pending' END,
                    fired_at_ms = CASE status WHEN 'paused' THEN fired_at_ms ELSE NULL END,
                    updated_at_ms = ?8
                 WHERE id = ?1 AND status != 'running'",
                params![
                    id,
                    task.prompt.trim(),
                    task.action.as_str(),
                    task.payload,
                    task.schedule.as_str(),
                    task.due_at_ms,
                    task.recurrence,
                    now_ms(),
                ],
            )
            .map_err(|err| anyhow!("update task: {err}"))?;

        if changed == 0 {
            return Ok(None);
        }
        self.get_task(id)
    }

    pub fn pause_task(&self, id: i64) -> Result<bool> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET status = 'paused', updated_at_ms = ?2
                 WHERE id = ?1 AND status = 'pending'",
                params![id, now_ms()],
            )
            .map_err(|err| anyhow!("pause task: {err}"))?;
        Ok(changed > 0)
    }

    pub fn resume_task(&self, id: i64, due_at_ms: Option<i64>) -> Result<Option<ScheduledTask>> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET
                    status = 'pending',
                    due_at_ms = COALESCE(?2, due_at_ms),
                    fired_at_ms = NULL,
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'paused'",
                params![id, due_at_ms, now_ms()],
            )
            .map_err(|err| anyhow!("resume task: {err}"))?;

        if changed == 0 {
            return Ok(None);
        }
        self.get_task(id)
    }

    pub fn cancel_task(&self, id: i64) -> Result<bool> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET status = ?2, updated_at_ms = ?3
                 WHERE id = ?1 AND status IN ('pending', 'paused')",
                params![id, TaskStatus::Cancelled.as_str(), now_ms()],
            )
            .map_err(|err| anyhow!("cancel task: {err}"))?;
        Ok(changed > 0)
    }

    pub fn delete_task(&self, id: i64) -> Result<bool> {
        let changed = self
            .lock()?
            .execute(
                "DELETE FROM scheduled_tasks WHERE id = ?1 AND status != 'running'",
                params![id],
            )
            .map_err(|err| anyhow!("delete task: {err}"))?;
        Ok(changed > 0)
    }

    /// Atomically claims the exact pending task snapshot selected by the scheduler.
    pub fn claim_task(
        &self,
        task: &ScheduledTask,
        next_due_ms: Option<i64>,
        fired_at_ms: i64,
    ) -> Result<bool> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET
                    due_at_ms = COALESCE(?2, due_at_ms),
                    status = 'running',
                    fired_at_ms = ?3,
                    fire_count = fire_count + 1,
                    last_error = NULL,
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'pending'
                    AND due_at_ms = ?4 AND updated_at_ms = ?5 AND due_at_ms <= ?3
                    AND prompt = ?6 AND action_kind = ?7 AND payload = ?8
                    AND schedule_kind = ?9 AND recurrence IS ?10
                    AND recurrence_start_ms IS ?11",
                params![
                    task.id,
                    next_due_ms,
                    fired_at_ms,
                    task.due_at_ms,
                    task.updated_at_ms,
                    task.prompt,
                    task.action.as_str(),
                    task.payload,
                    task.schedule.as_str(),
                    task.recurrence.as_deref(),
                    task.recurrence_start_ms
                ],
            )
            .map_err(|err| anyhow!("claim task: {err}"))?;
        Ok(changed > 0)
    }

    pub fn finish_task(
        &self,
        id: i64,
        schedule: ScheduleKind,
        now: i64,
        error: Option<&str>,
    ) -> Result<bool> {
        let changed = self
            .lock()?
            .execute(
                "UPDATE scheduled_tasks SET
                    status = CASE
                        WHEN ?2 = 'recurring' THEN 'pending'
                        WHEN ?4 IS NULL THEN 'done'
                        ELSE 'failed'
                    END,
                    last_error = ?4,
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'running'",
                params![id, schedule.as_str(), now, error],
            )
            .map_err(|err| anyhow!("finish task: {err}"))?;
        Ok(changed > 0)
    }

    /// Rolls a missed recurring task forward without running it.
    pub fn reschedule(&self, task: &ScheduledTask, next_due_ms: i64, now: i64) -> Result<()> {
        self.lock()?
            .execute(
                "UPDATE scheduled_tasks SET due_at_ms = ?2, status = 'pending',
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'pending'
                    AND due_at_ms = ?4 AND updated_at_ms = ?5
                    AND prompt = ?6 AND action_kind = ?7 AND payload = ?8
                    AND schedule_kind = ?9 AND recurrence IS ?10
                    AND recurrence_start_ms IS ?11",
                params![
                    task.id,
                    next_due_ms,
                    now,
                    task.due_at_ms,
                    task.updated_at_ms,
                    task.prompt,
                    task.action.as_str(),
                    task.payload,
                    task.schedule.as_str(),
                    task.recurrence.as_deref(),
                    task.recurrence_start_ms
                ],
            )
            .map_err(|err| anyhow!("reschedule task: {err}"))?;
        Ok(())
    }

    pub fn mark_missed(&self, task: &ScheduledTask, now: i64, reason: Option<&str>) -> Result<()> {
        self.lock()?
            .execute(
                "UPDATE scheduled_tasks SET status = 'missed', last_error = ?2,
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'pending'
                    AND due_at_ms = ?4 AND updated_at_ms = ?5
                    AND prompt = ?6 AND action_kind = ?7 AND payload = ?8
                    AND schedule_kind = ?9 AND recurrence IS ?10
                    AND recurrence_start_ms IS ?11",
                params![
                    task.id,
                    reason,
                    now,
                    task.due_at_ms,
                    task.updated_at_ms,
                    task.prompt,
                    task.action.as_str(),
                    task.payload,
                    task.schedule.as_str(),
                    task.recurrence.as_deref(),
                    task.recurrence_start_ms
                ],
            )
            .map_err(|err| anyhow!("mark task missed: {err}"))?;
        Ok(())
    }

    pub fn mark_pending_failed(
        &self,
        task: &ScheduledTask,
        now: i64,
        error: Option<&str>,
    ) -> Result<()> {
        self.lock()?
            .execute(
                "UPDATE scheduled_tasks SET status = 'failed', last_error = ?2,
                    updated_at_ms = ?3
                 WHERE id = ?1 AND status = 'pending'
                    AND due_at_ms = ?4 AND updated_at_ms = ?5
                    AND prompt = ?6 AND action_kind = ?7 AND payload = ?8
                    AND schedule_kind = ?9 AND recurrence IS ?10
                    AND recurrence_start_ms IS ?11",
                params![
                    task.id,
                    error,
                    now,
                    task.due_at_ms,
                    task.updated_at_ms,
                    task.prompt,
                    task.action.as_str(),
                    task.payload,
                    task.schedule.as_str(),
                    task.recurrence.as_deref(),
                    task.recurrence_start_ms
                ],
            )
            .map_err(|err| anyhow!("mark pending task failed: {err}"))?;
        Ok(())
    }

    pub fn recover_interrupted_tasks(&self, now: i64) -> Result<Vec<i64>> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|err| anyhow!("start interrupted-task recovery: {err}"))?;
        let ids = {
            let mut statement = tx
                .prepare("SELECT id FROM scheduled_tasks WHERE status = 'running'")
                .map_err(|err| anyhow!("prepare interrupted-task lookup: {err}"))?;
            let rows = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(|err| anyhow!("query interrupted tasks: {err}"))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|err| anyhow!("read interrupted task: {err}"))?
        };
        tx.execute(
            "UPDATE scheduled_tasks SET
                status = CASE WHEN schedule_kind = 'recurring' THEN 'pending' ELSE 'failed' END,
                last_error = 'Interrupted while Murmur was running; the action was not retried',
                updated_at_ms = ?1
             WHERE status = 'running'",
            params![now],
        )
        .map_err(|err| anyhow!("recover interrupted tasks: {err}"))?;
        tx.commit()
            .map_err(|err| anyhow!("commit interrupted-task recovery: {err}"))?;
        Ok(ids)
    }
}

fn table_columns(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn
        .prepare("PRAGMA table_info(scheduled_tasks)")
        .map_err(|err| anyhow!("read table info: {err}"))?;

    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| anyhow!("query table info: {err}"))?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(|err| anyhow!("read table column: {err}"))?);
    }
    Ok(columns)
}

fn map_task(row: &Row<'_>) -> rusqlite::Result<ScheduledTask> {
    let action: String = row.get(2)?;
    let schedule: String = row.get(4)?;
    let status: String = row.get(8)?;

    Ok(ScheduledTask {
        id: row.get(0)?,
        prompt: row.get(1)?,
        action: ActionKind::from_db(&action),
        payload: row.get(3)?,
        schedule: ScheduleKind::from_db(&schedule),
        due_at_ms: row.get(5)?,
        recurrence: row.get(6)?,
        recurrence_start_ms: row.get(7)?,
        status: TaskStatus::from_db(&status),
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
        fired_at_ms: row.get(11)?,
        fire_count: row.get(12)?,
        last_error: row.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields<'a>(prompt: &'a str, due_at_ms: i64) -> TaskFields<'a> {
        TaskFields {
            prompt,
            action: ActionKind::Remind,
            payload: prompt,
            schedule: ScheduleKind::OneTime,
            due_at_ms,
            recurrence: None,
        }
    }

    #[test]
    fn a_paused_task_cannot_be_claimed_from_a_stale_due_list() {
        let db = Database::open(":memory:").expect("database opens");
        let task = db
            .insert_task(&fields("remind me", 1))
            .expect("task inserts");
        assert!(db.pause_task(task.id).expect("task pauses"));

        assert!(!db
            .claim_task(&task, None, 2)
            .expect("stale claim is rejected"));
    }

    #[test]
    fn an_edited_task_cannot_be_claimed_from_a_stale_due_list() {
        let db = Database::open(":memory:").expect("database opens");
        let task = db
            .insert_task(&fields("original reminder", 1))
            .expect("task inserts");
        db.update_task(task.id, &fields("edited reminder", 1))
            .expect("task updates");

        assert!(!db
            .claim_task(&task, None, 2)
            .expect("stale claim is rejected"));
    }

    #[test]
    fn editing_a_recurring_task_reanchors_it_to_the_new_due_time() {
        let db = Database::open(":memory:").expect("database opens");
        let mut original = fields("monthly reminder", 1);
        original.schedule = ScheduleKind::Recurring;
        original.recurrence = Some("FREQ=MONTHLY");
        let task = db.insert_task(&original).expect("task inserts");
        assert_eq!(task.recurrence_start_ms, Some(task.created_at_ms));
        let mut edited = fields("monthly reminder", 31);
        edited.schedule = ScheduleKind::Recurring;
        edited.recurrence = Some("FREQ=MONTHLY");

        let updated = db
            .update_task(task.id, &edited)
            .expect("task updates")
            .expect("task exists");
        assert_eq!(updated.recurrence_start_ms, Some(31));
    }

    #[test]
    fn migration_backfills_recurring_anchor_from_current_due_time() {
        let conn = Connection::open_in_memory().expect("database opens");
        conn.execute_batch(
            "CREATE TABLE scheduled_tasks (
                id INTEGER PRIMARY KEY,
                prompt TEXT NOT NULL,
                action_kind TEXT NOT NULL,
                payload TEXT NOT NULL,
                schedule_kind TEXT NOT NULL,
                due_at_ms INTEGER NOT NULL,
                recurrence TEXT,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                fired_at_ms INTEGER,
                fire_count INTEGER NOT NULL,
                last_error TEXT
             );
             INSERT INTO scheduled_tasks
                (id, prompt, action_kind, payload, schedule_kind, due_at_ms,
                 recurrence, status, created_at_ms, updated_at_ms, fire_count)
             VALUES (1, 'monthly reminder', 'remind', '', 'recurring', 123,
                     'FREQ=MONTHLY', 'pending', 1, 1, 0);",
        )
        .expect("legacy task is created");
        let db = Database {
            conn: Mutex::new(conn),
        };

        db.migrate().expect("migration succeeds");
        let task = db.get_task(1).expect("task loads").expect("task exists");
        assert_eq!(task.recurrence_start_ms, Some(123));
    }

    #[test]
    fn active_tasks_sort_ahead_of_old_completed_tasks() {
        let db = Database::open(":memory:").expect("database opens");
        let old = db
            .insert_task(&fields("old task", 1))
            .expect("task inserts");
        assert!(db.claim_task(&old, None, 2).expect("task claims"));
        assert!(db
            .finish_task(old.id, ScheduleKind::OneTime, 3, None)
            .expect("task completes"));
        db.insert_task(&fields("active task", 10))
            .expect("active task inserts");

        let tasks = db.list_tasks(200).expect("tasks list");
        assert_eq!(tasks[0].prompt, "active task");
        assert_eq!(tasks[1].prompt, "old task");
    }

    #[test]
    fn interrupted_one_time_tasks_are_marked_failed_on_recovery() {
        let db = Database::open(":memory:").expect("database opens");
        let task = db
            .insert_task(&fields("remind me", 1))
            .expect("task inserts");
        assert!(db.claim_task(&task, None, 2).expect("task claims"));

        assert_eq!(
            db.recover_interrupted_tasks(3).expect("recovery"),
            vec![task.id]
        );
        let recovered = db
            .get_task(task.id)
            .expect("task lookup")
            .expect("task exists");
        assert_eq!(recovered.status, TaskStatus::Failed);
        assert!(recovered.last_error.is_some());
    }

    #[test]
    fn interrupted_recurring_tasks_resume_at_the_next_occurrence() {
        let db = Database::open(":memory:").expect("database opens");
        let mut fields = fields("daily reminder", 1);
        fields.schedule = ScheduleKind::Recurring;
        fields.recurrence = Some("FREQ=DAILY");
        let task = db.insert_task(&fields).expect("task inserts");
        assert!(db.claim_task(&task, Some(100), 2).expect("task claims"));

        assert_eq!(
            db.recover_interrupted_tasks(3).expect("recovery"),
            vec![task.id]
        );
        let recovered = db
            .get_task(task.id)
            .expect("task lookup")
            .expect("task exists");
        assert_eq!(recovered.status, TaskStatus::Pending);
        assert_eq!(recovered.due_at_ms, 100);
        assert!(recovered.last_error.is_some());
    }
}
