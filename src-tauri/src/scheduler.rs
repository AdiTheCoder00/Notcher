//! Scheduled tasks: the SQLite table, the recurrence rules and the tokio
//! scheduler that executes each task's action.
//!
//! Only text lives here. Audio and screenshots are never written to the
//! database, and finished tasks keep their prompt but nothing else.
//!
//! Persistence is the whole restart story: every task is in SQLite, so a
//! fresh process recovers interrupted work before resuming the table. One-time tasks that
//! came due while Murmur was closed are fired when they are at most five
//! minutes late; older ones are marked `missed` instead of surprising the user
//! with a burst of stale actions. Recurring tasks that far behind are rolled
//! forward to their next occurrence.

use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{
    DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Weekday,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::{now_ms, TaskFields};
use crate::gemini::{self, PlannedAction};
use crate::state::AppState;

/// Emitted to every window when a task comes due or changes state.
pub const TASK_DUE_EVENT: &str = "task://due";

/// How late a one-time task may be and still fire after a restart.
pub const MISSED_WINDOW_MS: i64 = 5 * 60 * 1000;

const TICK: Duration = Duration::from_secs(1);
const DUE_LIMIT: i64 = 50;
const CATCH_UP_LIMIT: i64 = 500;
/// A due task must never be held up by a slow model: after this the stored
/// plan runs instead.
const EXECUTE_PLAN_TIMEOUT: Duration = Duration::from_secs(8);

// ---------------------------------------------------------------------------
// Task model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    OpenApp,
    OpenUrl,
    RunSearch,
    Remind,
    TypeText,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::OpenApp => "open_app",
            ActionKind::OpenUrl => "open_url",
            ActionKind::RunSearch => "run_search",
            ActionKind::Remind => "remind",
            ActionKind::TypeText => "type_text",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "open_app" => ActionKind::OpenApp,
            "open_url" => ActionKind::OpenUrl,
            "run_search" => ActionKind::RunSearch,
            "type_text" => ActionKind::TypeText,
            _ => ActionKind::Remind,
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "open_app" => Ok(ActionKind::OpenApp),
            "open_url" => Ok(ActionKind::OpenUrl),
            "run_search" => Ok(ActionKind::RunSearch),
            "remind" => Ok(ActionKind::Remind),
            "type_text" => Ok(ActionKind::TypeText),
            other => Err(anyhow!("unknown task action: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleKind {
    OneTime,
    Recurring,
}

impl ScheduleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleKind::OneTime => "one_time",
            ScheduleKind::Recurring => "recurring",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "recurring" => ScheduleKind::Recurring,
            _ => ScheduleKind::OneTime,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Paused,
    Running,
    Done,
    Cancelled,
    Missed,
    Failed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Paused => "paused",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Missed => "missed",
            TaskStatus::Failed => "failed",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "paused" => TaskStatus::Paused,
            "running" => TaskStatus::Running,
            "done" => TaskStatus::Done,
            "cancelled" => TaskStatus::Cancelled,
            "missed" => TaskStatus::Missed,
            "failed" => TaskStatus::Failed,
            _ => TaskStatus::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: i64,
    pub prompt: String,
    pub action: ActionKind,
    pub payload: String,
    pub schedule: ScheduleKind,
    pub due_at_ms: i64,
    pub recurrence: Option<String>,
    pub recurrence_start_ms: Option<i64>,
    pub status: TaskStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub fired_at_ms: Option<i64>,
    pub fire_count: i64,
    pub last_error: Option<String>,
}

// ---------------------------------------------------------------------------
// Recurrence rules
// ---------------------------------------------------------------------------

/// The subset of RFC 5545 recurrence rules the assistant produces. Keeping the
/// parser small is deliberate: the model is told exactly which parts are
/// supported, and anything else fails loudly at creation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurrence {
    frequency: Frequency,
    interval: u32,
    by_day: Vec<Weekday>,
    by_hour: Option<u32>,
    by_minute: Option<u32>,
    by_month_day: Option<u32>,
}

impl Recurrence {
    pub fn parse(rule: &str) -> Result<Self> {
        let rule = rule.trim().trim_start_matches("RRULE:");
        if rule.is_empty() {
            return Err(anyhow!("the recurrence rule is empty"));
        }

        let mut frequency = None;
        let mut interval = 1;
        let mut by_day = Vec::new();
        let mut by_hour = None;
        let mut by_minute = None;
        let mut by_month_day = None;

        for part in rule.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| anyhow!("bad recurrence part: {part}"))?;

            match key.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    frequency = Some(match value.trim().to_ascii_uppercase().as_str() {
                        "HOURLY" => Frequency::Hourly,
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        other => {
                            return Err(anyhow!(
                                "unsupported recurrence frequency: {other}; use HOURLY, DAILY, WEEKLY or MONTHLY"
                            ))
                        }
                    })
                }
                "INTERVAL" => {
                    interval = value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| anyhow!("bad recurrence interval: {value}"))?;
                    if interval == 0 {
                        return Err(anyhow!("a recurrence interval must be at least 1"));
                    }
                }
                "BYDAY" => {
                    by_day.clear();
                    for day in value.split(',') {
                        by_day.push(parse_weekday(day.trim())?);
                    }
                }
                "BYHOUR" => {
                    let hour = parse_number(value, "hour")?;
                    if hour > 23 {
                        return Err(anyhow!("BYHOUR must be between 0 and 23"));
                    }
                    by_hour = Some(hour);
                }
                "BYMINUTE" => {
                    let minute = parse_number(value, "minute")?;
                    if minute > 59 {
                        return Err(anyhow!("BYMINUTE must be between 0 and 59"));
                    }
                    by_minute = Some(minute);
                }
                "BYMONTHDAY" => {
                    let day = parse_number(value, "month day")?;
                    if !(1..=31).contains(&day) {
                        return Err(anyhow!("BYMONTHDAY must be between 1 and 31"));
                    }
                    by_month_day = Some(day);
                }
                other => return Err(anyhow!("unsupported recurrence part: {other}")),
            }
        }

        let frequency = frequency.ok_or_else(|| anyhow!("the recurrence rule has no FREQ"))?;
        if frequency == Frequency::Monthly && !by_day.is_empty() {
            return Err(anyhow!("BYDAY is not supported with monthly recurrence"));
        }
        Ok(Self {
            frequency,
            interval,
            by_day,
            by_hour,
            by_minute,
            by_month_day,
        })
    }

    /// Next occurrence strictly after `after`, in local wall-clock time.
    ///
    /// This is pure and timezone-free so it can be unit tested without the
    /// machine's clock or locale.
    #[cfg(test)]
    pub fn next_after_local(&self, after: NaiveDateTime) -> Option<NaiveDateTime> {
        self.next_after_local_from(after, after)
    }

    pub fn next_after_local_from(
        &self,
        after: NaiveDateTime,
        anchor: NaiveDateTime,
    ) -> Option<NaiveDateTime> {
        match self.frequency {
            Frequency::Hourly => self.next_hourly(after, anchor),
            Frequency::Daily => self.next_daily(after, anchor),
            Frequency::Weekly => self.next_weekly(after, anchor),
            Frequency::Monthly => self.next_monthly(after, anchor),
        }
    }

    /// Next occurrence as a Unix millisecond timestamp in the local timezone.
    pub fn next_after_ms(&self, after_ms: i64) -> Option<i64> {
        self.next_after_ms_from(after_ms, after_ms)
    }

    pub fn next_after_ms_from(&self, after_ms: i64, anchor_ms: i64) -> Option<i64> {
        let after = Local.timestamp_millis_opt(after_ms).single()?.naive_local();
        let anchor = Local
            .timestamp_millis_opt(anchor_ms)
            .single()?
            .naive_local();
        let next = self.next_after_local_from(after, anchor)?;

        Local
            .from_local_datetime(&next)
            .earliest()
            // A time that does not exist because the clock jumped forward is
            // moved to the same wall-clock minute after the jump.
            .or_else(|| {
                Local
                    .from_local_datetime(&(next + chrono::Duration::hours(1)))
                    .earliest()
            })
            .map(|datetime| datetime.timestamp_millis())
    }

    fn next_hourly(&self, after: NaiveDateTime, anchor: NaiveDateTime) -> Option<NaiveDateTime> {
        let minute = self.by_minute.unwrap_or(anchor.minute());
        let mut candidate = anchor.date().and_hms_opt(anchor.hour(), minute, 0)?;

        for _ in 0..MAX_STEPS {
            if candidate > after {
                return Some(candidate);
            }
            candidate =
                candidate.checked_add_signed(chrono::Duration::hours(self.interval as i64))?;
        }
        None
    }

    fn next_daily(&self, after: NaiveDateTime, anchor: NaiveDateTime) -> Option<NaiveDateTime> {
        let hour = self.by_hour.unwrap_or(anchor.hour());
        let minute = self.by_minute.unwrap_or(anchor.minute());
        let mut date = anchor.date();

        for _ in 0..MAX_STEPS {
            let candidate = date.and_hms_opt(hour, minute, 0)?;
            if candidate > after {
                return Some(candidate);
            }
            date = date.checked_add_days(Days::new(self.interval as u64))?;
        }
        None
    }

    fn next_weekly(
        &self,
        after: NaiveDateTime,
        anchor_date: NaiveDateTime,
    ) -> Option<NaiveDateTime> {
        let hour = self.by_hour.unwrap_or(anchor_date.hour());
        let minute = self.by_minute.unwrap_or(anchor_date.minute());
        let allowed: Vec<Weekday> = if self.by_day.is_empty() {
            vec![anchor_date.weekday()]
        } else {
            self.by_day.clone()
        };

        let anchor = anchor_date.date().checked_sub_days(Days::new(
            anchor_date.weekday().num_days_from_monday() as u64,
        ))?;
        let interval = self.interval as i64;
        let mut date = anchor_date.date();

        for _ in 0..MAX_STEPS {
            let week_index = (date - anchor).num_days() / 7;
            let in_cycle = week_index >= 0 && week_index % interval == 0;
            if in_cycle && allowed.iter().any(|day| *day == date.weekday()) {
                let candidate = date.and_hms_opt(hour, minute, 0)?;
                if candidate > after {
                    return Some(candidate);
                }
            }
            date = date.checked_add_days(Days::new(1))?;
        }
        None
    }

    fn next_monthly(&self, after: NaiveDateTime, anchor: NaiveDateTime) -> Option<NaiveDateTime> {
        let hour = self.by_hour.unwrap_or(anchor.hour());
        let minute = self.by_minute.unwrap_or(anchor.minute());
        let desired_day = self.by_month_day.unwrap_or(anchor.day());
        let mut year = anchor.year();
        let mut month = anchor.month();

        for _ in 0..MAX_STEPS {
            let day = desired_day.min(last_day_of_month(year, month));
            let candidate =
                NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, 0)?;
            if candidate > after {
                return Some(candidate);
            }

            let next = i64::from(year) * 12 + i64::from(month) - 1 + i64::from(self.interval);
            year = i32::try_from(next.div_euclid(12)).ok()?;
            month = u32::try_from(next.rem_euclid(12) + 1).ok()?;
        }
        None
    }
}

const MAX_STEPS: usize = 5_000;

fn parse_number(value: &str, label: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow!("bad recurrence {label}: {value}"))
}

fn parse_weekday(value: &str) -> Result<Weekday> {
    match value.trim().to_ascii_uppercase().as_str() {
        "MO" => Ok(Weekday::Mon),
        "TU" => Ok(Weekday::Tue),
        "WE" => Ok(Weekday::Wed),
        "TH" => Ok(Weekday::Thu),
        "FR" => Ok(Weekday::Fri),
        "SA" => Ok(Weekday::Sat),
        "SU" => Ok(Weekday::Sun),
        other => Err(anyhow!("unknown BYDAY value: {other}")),
    }
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };

    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .and_then(|date| date.checked_sub_days(Days::new(1)))
        .map(|date| date.day())
        .unwrap_or(28)
}

/// Parses the `at` value Gemini produces for one-time tasks. Date-times without
/// an offset are interpreted in the machine's local timezone.
pub fn parse_local_datetime(value: &str) -> Result<NaiveDateTime> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("the task has no date or time"));
    }

    if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
        return Ok(datetime.with_timezone(&Local).naive_local());
    }

    for format in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(datetime) = NaiveDateTime::parse_from_str(value, format) {
            return Ok(datetime);
        }
    }

    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date
            .and_hms_opt(9, 0, 0)
            .ok_or_else(|| anyhow!("invalid date: {value}"));
    }

    Err(anyhow!(
        "could not read the date and time {value:?}; expected an ISO 8601 local datetime"
    ))
}

pub fn local_datetime_to_ms(datetime: NaiveDateTime) -> Result<i64> {
    Local
        .from_local_datetime(&datetime)
        .earliest()
        .map(|value| value.timestamp_millis())
        .ok_or_else(|| anyhow!("the date and time do not exist in this timezone"))
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// What to do with a pending task that is already due when the process starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchUp {
    /// Late enough to still be useful: run it now.
    Fire,
    /// A one-time task that is too old: do not run it.
    MarkMissed,
    /// A recurring task that is too old: jump to its next occurrence.
    Advance,
}

pub fn catch_up_decision(
    schedule: ScheduleKind,
    due_at_ms: i64,
    now_ms: i64,
    window_ms: i64,
) -> CatchUp {
    if due_at_ms > now_ms {
        return CatchUp::Advance;
    }

    if now_ms.saturating_sub(due_at_ms) <= window_ms {
        return CatchUp::Fire;
    }

    match schedule {
        ScheduleKind::OneTime => CatchUp::MarkMissed,
        ScheduleKind::Recurring => CatchUp::Advance,
    }
}

/// Starts the scheduler. The first thing it does is catch up on tasks that
/// came due while the app was closed, then it ticks once a second.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let recovered = {
            let state = app.state::<AppState>();
            state.db.recover_interrupted_tasks(now_ms())
        };
        match recovered {
            Ok(ids) => {
                for id in ids {
                    emit_latest(&app, id);
                }
            }
            Err(err) => eprintln!("[scheduler] interrupted-task recovery failed: {err:#}"),
        }

        if let Err(err) = catch_up_on_start(&app).await {
            eprintln!("[scheduler] startup catch-up failed: {err:#}");
        }

        let start = tokio::time::Instant::now() + TICK;
        let mut ticker = tokio::time::interval_at(start, TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            if let Err(err) = run_due(&app).await {
                eprintln!("[scheduler] tick failed: {err:#}");
            }
        }
    });
}

async fn catch_up_on_start(app: &AppHandle) -> Result<()> {
    let now = now_ms();
    let due = {
        let state = app.state::<AppState>();
        state.db.due_tasks(now, CATCH_UP_LIMIT)?
    };

    for task in due {
        process_due_task(app, task).await?;
    }

    Ok(())
}

async fn run_due(app: &AppHandle) -> Result<()> {
    let now = now_ms();
    let due = {
        let state = app.state::<AppState>();
        state.db.due_tasks(now, DUE_LIMIT)?
    };

    for task in due {
        if let Err(err) = process_due_task(app, task.clone()).await {
            eprintln!("[scheduler] task {} could not start: {err:#}", task.id);
            record_failure(app, &task, now, &err);
        }
    }

    Ok(())
}

async fn process_due_task(app: &AppHandle, task: ScheduledTask) -> Result<()> {
    let now = now_ms();
    match catch_up_decision(task.schedule, task.due_at_ms, now, MISSED_WINDOW_MS) {
        CatchUp::Fire => {
            if let Err(err) = fire_task(app, task.clone(), now).await {
                record_failure(app, &task, now, &err);
            }
        }
        CatchUp::MarkMissed => {
            let reason = "Missed while Murmur was not running";
            {
                let state = app.state::<AppState>();
                state.db.mark_missed(&task, now, Some(reason))?;
            }
            emit_latest(app, task.id);
        }
        CatchUp::Advance => {
            if task.schedule == ScheduleKind::Recurring {
                match next_due_ms(&task, now) {
                    Ok(next) => {
                        let state = app.state::<AppState>();
                        state.db.reschedule(&task, next, now)?;
                    }
                    Err(err) => record_failure(app, &task, now, &err),
                }
                emit_latest(app, task.id);
            }
        }
    }
    Ok(())
}

/// Claims a task in the database before running its action, so another tick
/// cannot fire it twice. Startup recovery handles claims left by a crash.
async fn fire_task(app: &AppHandle, task: ScheduledTask, fired_at: i64) -> Result<()> {
    let next_due = if task.schedule == ScheduleKind::Recurring {
        Some(next_due_ms(&task, fired_at)?)
    } else {
        None
    };

    {
        let state = app.state::<AppState>();
        if !state.db.claim_task(&task, next_due, fired_at)? {
            return Ok(());
        }
    }

    if let Err(err) = crate::notch::show(
        app,
        crate::notch::NotchState::Thinking,
        Some(&format!("Running: {}", task.prompt)),
    ) {
        eprintln!("[scheduler] could not surface the task: {err:#}");
    }

    // The UI reloads its list from this event, including the advanced due time.
    emit_latest(app, task.id);

    match run_action(app, &task).await {
        Ok(summary) => {
            let state = app.state::<AppState>();
            if !state
                .db
                .finish_task(task.id, task.schedule, fired_at, None)?
            {
                return Err(anyhow!("task {} lost its running claim", task.id));
            }
            emit_latest(app, task.id);
            if let Err(err) =
                crate::notch::show(app, crate::notch::NotchState::Answer, Some(&summary))
            {
                eprintln!("[scheduler] could not report the result: {err:#}");
            }
        }
        Err(err) => {
            let message = format!("{err:#}");
            eprintln!("[scheduler] task {} action failed: {message}", task.id);

            {
                let state = app.state::<AppState>();
                if !state
                    .db
                    .finish_task(task.id, task.schedule, fired_at, Some(&message))?
                {
                    return Err(anyhow!("task {} lost its running claim", task.id));
                }
            }

            let _ = crate::notch::show(
                app,
                crate::notch::NotchState::Error,
                Some(&format!("Task failed: {message}")),
            );
            emit_latest(app, task.id);
        }
    }

    crate::notch::idle_later(app.clone(), Duration::from_millis(4000));
    Ok(())
}

async fn run_action(app: &AppHandle, task: &ScheduledTask) -> Result<String> {
    let (action, payload) = resolve_action(task, plan_action(task).await);
    let summary = action_summary(action, &payload);

    let handle = app.clone();
    let prompt = task.prompt.clone();

    // Opening apps, typing and toasts can block briefly; keep them off the
    // async runtime's worker threads.
    tauri::async_runtime::spawn_blocking(move || {
        crate::actions::execute(&handle, action, &payload, &prompt)
    })
    .await
    .map_err(|err| anyhow!("task action panicked: {err}"))??;

    Ok(summary)
}

/// Asks Gemini for the concrete thing to run now. `None` means the task runs
/// exactly as scheduled — no key, no network, a timeout or a bad reply.
async fn plan_action(task: &ScheduledTask) -> Option<PlannedAction> {
    let client = match gemini::GeminiClient::from_stored_key() {
        Ok(client) => client,
        Err(_) => return None,
    };

    match tokio::time::timeout(
        EXECUTE_PLAN_TIMEOUT,
        client.execute_task(&task.prompt, task.action, &task.payload),
    )
    .await
    {
        Ok(Ok(planned)) => Some(planned),
        Ok(Err(err)) => {
            eprintln!(
                "[scheduler] task {} keeps its scheduled action: {err}",
                task.id
            );
            None
        }
        Err(_) => {
            eprintln!(
                "[scheduler] task {} action planning timed out; using the scheduled action",
                task.id
            );
            None
        }
    }
}

/// The stored plan is the user's intent, so a model reply may refine only its
/// payload. A different kind, or an empty payload, is ignored.
fn resolve_action(task: &ScheduledTask, planned: Option<PlannedAction>) -> (ActionKind, String) {
    match planned {
        Some(planned) if planned.action == task.action && !planned.payload.trim().is_empty() => {
            (planned.action, planned.payload)
        }
        Some(planned) => {
            eprintln!(
                "[scheduler] task {} ignored a plan that changed {} into {}",
                task.id,
                task.action.as_str(),
                planned.action.as_str()
            );
            (task.action, task.payload.clone())
        }
        None => (task.action, task.payload.clone()),
    }
}

/// One short line for the notch; the payload is the interesting part.
fn action_summary(action: ActionKind, payload: &str) -> String {
    let short: String = payload.trim().chars().take(80).collect();
    match action {
        ActionKind::OpenApp | ActionKind::OpenUrl => format!("Opened {short}"),
        ActionKind::RunSearch => format!("Searched for {short}"),
        ActionKind::Remind => format!("Reminded: {short}"),
        ActionKind::TypeText => format!("Typed {} characters", payload.chars().count()),
    }
}

fn next_due_ms(task: &ScheduledTask, after_ms: i64) -> Result<i64> {
    let rule = task
        .recurrence
        .as_deref()
        .ok_or_else(|| anyhow!("recurring task {} has no recurrence rule", task.id))?;
    let recurrence = Recurrence::parse(rule)?;
    recurrence
        .next_after_ms_from(after_ms, task.recurrence_start_ms.unwrap_or(task.due_at_ms))
        .ok_or_else(|| anyhow!("the recurrence rule has no next occurrence"))
}

/// Used when resuming a paused task: past one-time tasks run shortly instead of
/// immediately, and recurring tasks jump to their next occurrence.
pub fn due_after_resume(task: &ScheduledTask, now: i64) -> i64 {
    match task.schedule {
        ScheduleKind::Recurring => next_due_ms(task, now).unwrap_or(task.due_at_ms),
        ScheduleKind::OneTime if task.due_at_ms <= now => now + 1_000,
        ScheduleKind::OneTime => task.due_at_ms,
    }
}

fn record_failure(app: &AppHandle, task: &ScheduledTask, when: i64, err: &anyhow::Error) {
    let message = format!("{err:#}");
    {
        let state = app.state::<AppState>();
        if let Err(db_err) = state.db.mark_pending_failed(task, when, Some(&message)) {
            eprintln!(
                "[scheduler] could not mark task {} failed: {db_err:#}",
                task.id
            );
        }
    }
    emit_latest(app, task.id);
}

fn emit_latest(app: &AppHandle, id: i64) {
    let latest = {
        let state = app.state::<AppState>();
        state.db.get_task(id).ok().flatten()
    };

    if let Some(task) = latest {
        if let Err(err) = app.emit(TASK_DUE_EVENT, &task) {
            eprintln!("[scheduler] could not emit task {}: {err}", task.id);
        }
    }
}

/// Builds a validated task for the voice path. Kept here so the Gemini layer
/// and the commands agree on what a legal schedule is.
pub fn task_fields<'a>(
    prompt: &'a str,
    action: ActionKind,
    payload: &'a str,
    recurrence: Option<&'a str>,
    due_at_ms: i64,
) -> Result<TaskFields<'a>> {
    if prompt.trim().is_empty() {
        return Err(anyhow!("a task needs a prompt"));
    }
    if payload.trim().is_empty() && action != ActionKind::Remind {
        return Err(anyhow!("the {} action needs a payload", action.as_str()));
    }

    let backend = recurrence.map(str::trim).filter(|rule| !rule.is_empty());
    if let Some(rule) = backend {
        Recurrence::parse(rule)?;
    }

    Ok(TaskFields {
        prompt: prompt.trim(),
        action,
        payload: payload.trim(),
        schedule: if backend.is_some() {
            ScheduleKind::Recurring
        } else {
            ScheduleKind::OneTime
        },
        due_at_ms,
        recurrence: backend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|date| date.and_hms_opt(hour, minute, 0))
            .expect("valid test date")
    }

    #[test]
    fn daily_rule_uses_by_hour_and_minute() {
        let rule = Recurrence::parse("FREQ=DAILY;BYHOUR=9;BYMINUTE=0").expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 11, 7, 10, 0)),
            Some(at(2026, 11, 8, 9, 0))
        );
    }

    #[test]
    fn daily_interval_skips_days() {
        let rule =
            Recurrence::parse("FREQ=DAILY;INTERVAL=3;BYHOUR=8;BYMINUTE=30").expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 11, 7, 9, 0)),
            Some(at(2026, 11, 10, 8, 30))
        );
    }

    #[test]
    fn weekly_rule_finds_next_allowed_day() {
        // 2026-11-07 is a Saturday.
        let rule = Recurrence::parse("FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=18;BYMINUTE=30")
            .expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 11, 7, 12, 0)),
            Some(at(2026, 11, 9, 18, 30))
        );
    }

    #[test]
    fn weekly_interval_skips_whole_weeks() {
        let rule = Recurrence::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO;BYHOUR=9;BYMINUTE=0")
            .expect("rule parses");
        // Monday 2026-11-02 is in the anchor week; the next cycle is 11-16.
        assert_eq!(
            rule.next_after_local(at(2026, 11, 7, 12, 0)),
            Some(at(2026, 11, 16, 9, 0))
        );
    }

    #[test]
    fn daily_interval_catch_up_keeps_its_original_phase() {
        let rule =
            Recurrence::parse("FREQ=DAILY;INTERVAL=3;BYHOUR=9;BYMINUTE=0").expect("rule parses");
        assert_eq!(
            rule.next_after_local_from(at(2026, 11, 11, 10, 0), at(2026, 11, 7, 9, 0)),
            Some(at(2026, 11, 13, 9, 0))
        );
    }

    #[test]
    fn weekly_interval_catch_up_keeps_its_original_phase() {
        let rule = Recurrence::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO;BYHOUR=9;BYMINUTE=0")
            .expect("rule parses");
        assert_eq!(
            rule.next_after_local_from(at(2026, 11, 23, 10, 0), at(2026, 11, 7, 12, 0)),
            Some(at(2026, 11, 30, 9, 0))
        );
    }

    #[test]
    fn monthly_rule_uses_month_day() {
        let rule = Recurrence::parse("FREQ=MONTHLY;BYMONTHDAY=15;BYHOUR=8;BYMINUTE=0")
            .expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 11, 20, 12, 0)),
            Some(at(2026, 12, 15, 8, 0))
        );
    }

    #[test]
    fn monthly_rule_clamps_short_months() {
        let rule = Recurrence::parse("FREQ=MONTHLY;BYMONTHDAY=31;BYHOUR=8;BYMINUTE=0")
            .expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 1, 31, 9, 0)),
            Some(at(2026, 2, 28, 8, 0))
        );
    }

    #[test]
    fn monthly_rule_without_month_day_keeps_its_creation_day() {
        let created = Local
            .from_local_datetime(&at(2026, 1, 31, 9, 0))
            .single()
            .expect("valid local creation time")
            .timestamp_millis();
        let after = Local
            .from_local_datetime(&at(2026, 2, 28, 8, 0))
            .single()
            .expect("valid local due time")
            .timestamp_millis();
        let task = ScheduledTask {
            id: 1,
            prompt: "monthly reminder".into(),
            action: ActionKind::Remind,
            payload: "monthly reminder".into(),
            schedule: ScheduleKind::Recurring,
            due_at_ms: after,
            recurrence: Some("FREQ=MONTHLY;BYHOUR=8;BYMINUTE=0".into()),
            recurrence_start_ms: Some(created),
            status: TaskStatus::Pending,
            created_at_ms: created,
            updated_at_ms: created,
            fired_at_ms: None,
            fire_count: 1,
            last_error: None,
        };

        let next = next_due_ms(&task, after).expect("next occurrence");
        let next = Local
            .timestamp_millis_opt(next)
            .single()
            .expect("local time");
        assert_eq!(next.date_naive(), at(2026, 3, 31, 8, 0).date());
    }

    #[test]
    fn editing_monthly_due_time_reanchors_future_occurrences() {
        let created = Local
            .from_local_datetime(&at(2026, 1, 10, 9, 0))
            .single()
            .expect("valid original creation time")
            .timestamp_millis();
        let edited_due = Local
            .from_local_datetime(&at(2026, 2, 28, 8, 0))
            .single()
            .expect("valid edited due time")
            .timestamp_millis();
        let task = ScheduledTask {
            id: 1,
            prompt: "monthly reminder".into(),
            action: ActionKind::Remind,
            payload: "monthly reminder".into(),
            schedule: ScheduleKind::Recurring,
            due_at_ms: edited_due,
            recurrence: Some("FREQ=MONTHLY;BYHOUR=8;BYMINUTE=0".into()),
            recurrence_start_ms: Some(edited_due),
            status: TaskStatus::Pending,
            created_at_ms: created,
            updated_at_ms: created,
            fired_at_ms: None,
            fire_count: 0,
            last_error: None,
        };

        let next = next_due_ms(&task, edited_due).expect("next occurrence");
        let next = Local
            .timestamp_millis_opt(next)
            .single()
            .expect("local time");
        assert_eq!(next.date_naive(), at(2026, 3, 28, 8, 0).date());
    }

    #[test]
    fn hourly_rule_steps_by_interval() {
        let rule = Recurrence::parse("FREQ=HOURLY;INTERVAL=2;BYMINUTE=0").expect("rule parses");
        assert_eq!(
            rule.next_after_local(at(2026, 11, 7, 11, 30)),
            Some(at(2026, 11, 7, 13, 0))
        );
    }

    #[test]
    fn invalid_rules_are_rejected() {
        assert!(Recurrence::parse("").is_err());
        assert!(Recurrence::parse("FREQ=SOMETIMES").is_err());
        assert!(Recurrence::parse("FREQ=DAILY;INTERVAL=0").is_err());
        assert!(Recurrence::parse("FREQ=WEEKLY;BYDAY=XX").is_err());
        assert!(Recurrence::parse("FREQ=DAILY;BYHOUR=24").is_err());
        assert!(Recurrence::parse("FREQ=DAILY;COUNT=3").is_err());
        assert!(Recurrence::parse("FREQ=MONTHLY;BYDAY=MO").is_err());
    }

    #[test]
    fn parses_local_iso_datetime() {
        assert_eq!(
            parse_local_datetime("2026-11-08T09:30:00").expect("datetime parses"),
            at(2026, 11, 8, 9, 30)
        );
        assert_eq!(
            parse_local_datetime("2026-11-08 09:30").expect("datetime parses"),
            at(2026, 11, 8, 9, 30)
        );
    }

    #[test]
    fn catch_up_fires_tasks_inside_the_window() {
        let now = 1_000_000;
        assert_eq!(
            catch_up_decision(ScheduleKind::OneTime, now - 60_000, now, MISSED_WINDOW_MS),
            CatchUp::Fire
        );
        assert_eq!(
            catch_up_decision(
                ScheduleKind::OneTime,
                now - MISSED_WINDOW_MS,
                now,
                MISSED_WINDOW_MS
            ),
            CatchUp::Fire
        );
    }

    #[test]
    fn catch_up_marks_old_one_time_tasks_missed() {
        let now = 1_000_000;
        assert_eq!(
            catch_up_decision(
                ScheduleKind::OneTime,
                now - MISSED_WINDOW_MS - 1,
                now,
                MISSED_WINDOW_MS
            ),
            CatchUp::MarkMissed
        );
    }

    #[test]
    fn catch_up_advances_old_recurring_tasks() {
        let now = 1_000_000;
        assert_eq!(
            catch_up_decision(
                ScheduleKind::Recurring,
                now - MISSED_WINDOW_MS - 1,
                now,
                MISSED_WINDOW_MS
            ),
            CatchUp::Advance
        );
    }

    #[test]
    fn catch_up_leaves_future_tasks_alone() {
        let now = 1_000_000;
        assert_eq!(
            catch_up_decision(ScheduleKind::OneTime, now + 1, now, MISSED_WINDOW_MS),
            CatchUp::Advance
        );
    }

    #[test]
    fn resuming_an_overdue_one_time_task_runs_it_soon() {
        let task = ScheduledTask {
            id: 1,
            prompt: "test".into(),
            action: ActionKind::Remind,
            payload: "test".into(),
            schedule: ScheduleKind::OneTime,
            due_at_ms: 500,
            recurrence: None,
            recurrence_start_ms: None,
            status: TaskStatus::Paused,
            created_at_ms: 0,
            updated_at_ms: 0,
            fired_at_ms: None,
            fire_count: 0,
            last_error: None,
        };
        assert_eq!(due_after_resume(&task, 1_000), 2_000);
    }

    #[test]
    fn actions_that_need_a_target_reject_an_empty_payload() {
        assert!(task_fields("open Spotify", ActionKind::OpenApp, "  ", None, 1).is_err());
        assert!(task_fields("search the web", ActionKind::RunSearch, "", None, 1).is_err());
        // A reminder can fall back to its prompt, so it needs no payload.
        assert!(task_fields("stretch", ActionKind::Remind, "", None, 1).is_ok());
    }

    #[test]
    fn task_fields_marks_a_rule_as_recurring() {
        let fields = task_fields(
            "stand up",
            ActionKind::Remind,
            "stand up",
            Some("FREQ=HOURLY"),
            1,
        )
        .expect("valid task");

        assert_eq!(fields.schedule, ScheduleKind::Recurring);
        assert_eq!(fields.recurrence, Some("FREQ=HOURLY"));
    }

    #[test]
    fn a_due_task_keeps_its_scheduled_action_unless_the_payload_is_refined() {
        let task = ScheduledTask {
            id: 7,
            prompt: "open Spotify".into(),
            action: ActionKind::OpenApp,
            payload: "Spotify".into(),
            schedule: ScheduleKind::OneTime,
            due_at_ms: 0,
            recurrence: None,
            recurrence_start_ms: None,
            status: TaskStatus::Pending,
            created_at_ms: 0,
            updated_at_ms: 0,
            fired_at_ms: None,
            fire_count: 0,
            last_error: None,
        };

        let refined = PlannedAction {
            action: ActionKind::OpenApp,
            payload: "Spotify Premium".into(),
        };
        assert_eq!(
            resolve_action(&task, Some(refined)),
            (ActionKind::OpenApp, "Spotify Premium".to_string())
        );

        // A reminder must never turn into typing into the focused window.
        let changed = PlannedAction {
            action: ActionKind::TypeText,
            payload: "hello".into(),
        };
        assert_eq!(
            resolve_action(&task, Some(changed)),
            (ActionKind::OpenApp, "Spotify".to_string())
        );

        assert_eq!(
            resolve_action(&task, None),
            (ActionKind::OpenApp, "Spotify".to_string())
        );
    }
}
