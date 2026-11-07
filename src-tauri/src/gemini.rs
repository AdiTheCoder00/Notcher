//! Gemini access: the user's own API key from the Windows Credential Manager,
//! one multimodal client for gemini-2.5-flash, and errors callers can act on.

use std::fmt;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Local;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::loadout::{PersonaSettings, VoiceSettings};
use crate::scheduler::{
    local_datetime_to_ms, parse_local_datetime, ActionKind, Recurrence, ScheduleKind,
};

/// Credential Manager entry: service "murmur", account "gemini-api-key".
const SERVICE: &str = "murmur";
const ACCOUNT: &str = "gemini-api-key";

const MODEL: &str = "gemini-2.5-flash";
const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/models";
/// Dictation is interactive; waiting much longer than this is worse than failing.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Every failure the UI needs to distinguish. Messages are user-facing: they are
/// shown verbatim in the notch.
#[derive(Debug)]
pub enum GeminiError {
    MissingKey,
    KeyStore(String),
    InvalidKey,
    RateLimited,
    NoNetwork,
    Timeout,
    Rejected(String),
    Server(String),
    Decode(String),
}

impl fmt::Display for GeminiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GeminiError::MissingKey => {
                write!(f, "No Gemini API key yet — add one in the Murmur window.")
            }
            GeminiError::KeyStore(detail) => write!(
                f,
                "Could not use the Windows Credential Manager entry: {detail}"
            ),
            GeminiError::InvalidKey => {
                write!(
                    f,
                    "Gemini rejected the API key — replace it in the Murmur window."
                )
            }
            GeminiError::RateLimited => {
                write!(
                    f,
                    "Gemini rate limit reached — wait a moment and try again."
                )
            }
            GeminiError::NoNetwork => {
                write!(
                    f,
                    "No network connection — check your internet and try again."
                )
            }
            GeminiError::Timeout => write!(f, "Gemini took too long to answer — try again."),
            GeminiError::Rejected(detail) => write!(f, "Gemini rejected the request: {detail}"),
            GeminiError::Server(detail) => {
                write!(f, "Gemini is having trouble right now: {detail}")
            }
            GeminiError::Decode(detail) => {
                write!(f, "Could not read the Gemini response: {detail}")
            }
        }
    }
}

impl std::error::Error for GeminiError {}

// ---------------------------------------------------------------------------
// API key storage
// ---------------------------------------------------------------------------

pub fn store_api_key(key: &str) -> Result<(), GeminiError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|err| GeminiError::KeyStore(err.to_string()))?;
    entry
        .set_password(key)
        .map_err(|err| GeminiError::KeyStore(err.to_string()))
}

pub fn api_key() -> Result<Option<String>, GeminiError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|err| GeminiError::KeyStore(err.to_string()))?;

    match entry.get_password() {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key.trim().to_string())),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(GeminiError::KeyStore(err.to_string())),
    }
}

pub fn has_api_key() -> bool {
    matches!(api_key(), Ok(Some(_)))
}

pub fn clear_api_key() -> Result<(), GeminiError> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|err| GeminiError::KeyStore(err.to_string()))?;

    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(GeminiError::KeyStore(err.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
}

impl Part {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            inline_data: None,
            function_call: None,
        }
    }

    pub fn inline(mime_type: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            text: None,
            inline_data: Some(InlineData {
                mime_type: mime_type.into(),
                data: STANDARD.encode(bytes),
            }),
            function_call: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

/// A tool call returned by Gemini. `args` stays a JSON value because the
/// function declaration is the schema, not a Rust type on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    pub parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl Content {
    pub fn user(parts: Vec<Part>) -> Self {
        Self {
            parts,
            role: Some("user".to_string()),
        }
    }

    /// A previous answer, replayed to Gemini as conversation history.
    pub fn model(text: impl Into<String>) -> Self {
        Self {
            parts: vec![Part::text(text)],
            role: Some("model".to_string()),
        }
    }

    /// `systemInstruction` is a bare `Content`: no role, because only the
    /// conversation turns accept `user`/`model`.
    fn system(text: impl Into<String>) -> Self {
        Self {
            parts: vec![Part::text(text)],
            role: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    function_declarations: Vec<FunctionDeclaration>,
}

#[derive(Debug, Clone, Serialize)]
struct FunctionDeclaration {
    name: &'static str,
    description: &'static str,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerateRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<ToolConfig>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolConfig {
    function_calling_config: FunctionCallingConfig,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FunctionCallingConfig {
    mode: &'static str,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<Content>,
    finish_reason: Option<String>,
}

/// What the scheduler expects back from a spoken task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedTask {
    pub prompt: String,
    pub due_at_ms: i64,
    pub recurrence: Option<String>,
    pub schedule: ScheduleKind,
    pub action: ActionKind,
    pub payload: String,
}

/// The concrete action to run now that a task has come due. The kind always
/// matches what was scheduled; only the payload is allowed to be refined.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedAction {
    pub action: ActionKind,
    pub payload: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct GeminiClient {
    http: reqwest::Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, GeminiError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|err| {
                GeminiError::Rejected(format!("could not build the HTTP client: {err}"))
            })?;
        Ok(Self {
            http,
            api_key: api_key.into(),
        })
    }

    pub fn from_stored_key() -> Result<Self, GeminiError> {
        let key = api_key()?.ok_or(GeminiError::MissingKey)?;
        Self::new(key)
    }

    /// Transcribes a 16 kHz mono WAV clip. The audio never leaves memory.
    /// The Loadout's voice style is appended to the instruction, so the user's
    /// choice changes the actual prompt rather than only the settings screen.
    pub async fn transcribe(
        &self,
        wav: Vec<u8>,
        voice: &VoiceSettings,
    ) -> Result<String, GeminiError> {
        let prompt = format!("{TRANSCRIBE_PROMPT}\n{}", voice.dictation_guidance());
        let request = GenerateRequest {
            contents: vec![Content::user(vec![
                Part::inline("audio/wav", &wav),
                Part::text(prompt),
            ])],
            system_instruction: Some(Content::system(SYSTEM_INSTRUCTION)),
            generation_config: Some(GenerationConfig {
                temperature: 0.1,
                response_mime_type: None,
            }),
            tools: None,
            tool_config: None,
        };
        // Dictation is interactive: a second 30-second timeout retry leaves the
        // user staring at "Transcribing…" and may submit the same audio twice.
        let reply = self.send_once(&request).await?;
        Ok(reply.require_text()?.trim().to_string())
    }

    /// Answers about a circled screen region. `contents` already holds the
    /// previous turns of the conversation when the follow-up window is open.
    /// The Loadout persona is appended to the system instruction.
    pub async fn answer_about_circle(
        &self,
        contents: Vec<Content>,
        persona: &PersonaSettings,
    ) -> Result<String, GeminiError> {
        let instruction = format!("{CIRCLED_INSTRUCTION}\n{}", persona.answer_guidance());
        let request = GenerateRequest {
            contents,
            system_instruction: Some(Content::system(instruction)),
            generation_config: Some(GenerationConfig {
                temperature: 0.2,
                response_mime_type: None,
            }),
            tools: None,
            tool_config: None,
        };
        let reply = self.send(request).await?;
        reply.require_text()
    }

    /// Turns a spoken scheduling request into a validated task using Gemini
    /// function calling. The current local date, time and timezone are included
    /// so phrases like "tomorrow at nine" resolve against the user's clock, not
    /// the model's idea of UTC.
    pub async fn plan_task(&self, wav: Vec<u8>) -> Result<PlannedTask, GeminiError> {
        let now = Local::now();
        let now_ms = now.timestamp_millis();
        let timezone = iana_time_zone::get_timezone().unwrap_or_else(|_| now.offset().to_string());
        let local_datetime = now.format("%Y-%m-%dT%H:%M:%S").to_string();

        let context = format!(
            "Current local date and time: {local_datetime}. Timezone: {timezone} (UTC{}). \
             Unix time: {now_ms} ms. The audio is one request to schedule a task. \
             Resolve relative times against this local clock and call create_task exactly once.",
            now.format("%:z")
        );

        let request = GenerateRequest {
            contents: vec![Content::user(vec![
                Part::inline("audio/wav", &wav),
                Part::text(context),
            ])],
            system_instruction: Some(Content::system(SCHEDULE_INSTRUCTION)),
            generation_config: Some(GenerationConfig {
                temperature: 0.1,
                response_mime_type: None,
            }),
            tools: Some(vec![create_task_tool()]),
            // There is exactly one tool: force a call so the model cannot
            // answer with prose and leave the request unscheduled.
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig { mode: "ANY" },
            }),
        };

        let reply = self.send(request).await?;
        let call = reply.function_call().ok_or_else(|| {
            GeminiError::Decode(format!(
                "Gemini did not call create_task (reply: {})",
                reply.text().trim()
            ))
        })?;

        if call.name != "create_task" {
            return Err(GeminiError::Decode(format!(
                "Gemini called an unknown tool: {}",
                call.name
            )));
        }

        let args: CreateTaskArgs = serde_json::from_value(call.args.clone()).map_err(|err| {
            GeminiError::Decode(format!("unreadable create_task arguments: {err}"))
        })?;

        build_planned_task(args, now_ms)
    }

    /// Turns the stored plan into the payload to run right now.
    ///
    /// A task was planned from speech, when the exact text, query or reminder
    /// body may not have been known yet. The model may refine only the payload:
    /// changing the action kind would let a reminder start typing into whatever
    /// window happens to be focused, so that is rejected.
    pub async fn execute_task(
        &self,
        prompt: &str,
        action: ActionKind,
        payload: &str,
    ) -> Result<PlannedAction, GeminiError> {
        let now = Local::now();
        let context = format!(
            "This scheduled task is firing now. Current local date and time: {}. \
             Task: {prompt}. It was scheduled as {} with payload: {payload}. \
             Call run_action exactly once, keeping that kind, with the concrete payload to run now.",
            now.format("%Y-%m-%dT%H:%M:%S"),
            action.as_str(),
        );

        let request = GenerateRequest {
            contents: vec![Content::user(vec![Part::text(context)])],
            system_instruction: Some(Content::system(EXECUTE_INSTRUCTION)),
            generation_config: Some(GenerationConfig {
                temperature: 0.2,
                response_mime_type: None,
            }),
            tools: Some(vec![run_action_tool()]),
            tool_config: Some(ToolConfig {
                function_calling_config: FunctionCallingConfig { mode: "ANY" },
            }),
        };

        let reply = self.send(request).await?;
        let call = reply.function_call().ok_or_else(|| {
            GeminiError::Decode(format!(
                "Gemini did not call run_action (reply: {})",
                reply.text().trim()
            ))
        })?;

        if call.name != "run_action" {
            return Err(GeminiError::Decode(format!(
                "Gemini called an unknown tool: {}",
                call.name
            )));
        }

        let args: RunActionArgs = serde_json::from_value(call.args.clone()).map_err(|err| {
            GeminiError::Decode(format!("unreadable run_action arguments: {err}"))
        })?;

        build_planned_action(args, action, prompt)
    }

    /// One retry, and only for a timeout: a lost connection or a rejected key
    /// will fail the same way twice, so retrying those just doubles the wait.
    async fn send(&self, request: GenerateRequest) -> Result<GeminiReply, GeminiError> {
        match self.send_once(&request).await {
            Err(GeminiError::Timeout) => {
                eprintln!("[gemini] request timed out; retrying once");
                self.send_once(&request).await
            }
            other => other,
        }
    }

    async fn send_once(&self, request: &GenerateRequest) -> Result<GeminiReply, GeminiError> {
        let url = format!("{ENDPOINT}/{MODEL}:generateContent");
        let response = self
            .http
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(request)
            .send()
            .await
            .map_err(map_transport_error)?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| GeminiError::Decode(err.to_string()))?;

        if !status.is_success() {
            let detail = error_message(&body);
            return Err(classify(status, &detail));
        }

        let parsed: GenerateResponse =
            serde_json::from_str(&body).map_err(|err| GeminiError::Decode(err.to_string()))?;

        let candidate = parsed.candidates.into_iter().next().ok_or_else(|| {
            GeminiError::Rejected("the request returned no candidates".to_string())
        })?;

        Ok(GeminiReply {
            content: candidate.content.unwrap_or_else(|| Content {
                parts: Vec::new(),
                role: None,
            }),
            finish_reason: candidate.finish_reason,
        })
    }
}

/// The parts of one response the callers need: text for dictation and circle
/// answers, function calls for scheduling.
struct GeminiReply {
    content: Content,
    finish_reason: Option<String>,
}

impl GeminiReply {
    fn text(&self) -> String {
        self.content
            .parts
            .iter()
            .filter_map(|part| part.text.as_deref())
            .collect::<Vec<_>>()
            .join("")
    }

    fn require_text(&self) -> Result<String, GeminiError> {
        let text = self.text();
        if text.trim().is_empty() {
            return Err(GeminiError::Decode(format!(
                "no text in the response (finish reason: {})",
                self.finish_reason
                    .clone()
                    .unwrap_or_else(|| "unknown".into())
            )));
        }
        Ok(text)
    }

    fn function_call(&self) -> Option<&FunctionCall> {
        self.content
            .parts
            .iter()
            .find_map(|part| part.function_call.as_ref())
    }
}

#[derive(Debug, Deserialize)]
struct CreateTaskArgs {
    prompt: String,
    schedule: ScheduleArgs,
    action: ActionArgs,
}

#[derive(Debug, Deserialize)]
struct ScheduleArgs {
    kind: String,
    at: Option<String>,
    recurrence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActionArgs {
    kind: String,
    payload: Option<String>,
}

fn build_planned_task(args: CreateTaskArgs, now_ms: i64) -> Result<PlannedTask, GeminiError> {
    let prompt = args.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(GeminiError::Decode(
            "Gemini created a task without a prompt".to_string(),
        ));
    }

    let action = ActionKind::parse(&args.action.kind)
        .map_err(|err| GeminiError::Decode(format!("{err}")))?;
    let payload = args.action.payload.unwrap_or_default().trim().to_string();
    let payload = if payload.is_empty() {
        if action == ActionKind::Remind {
            prompt.clone()
        } else {
            return Err(GeminiError::Decode(format!(
                "the {} action needs a payload",
                action.as_str()
            )));
        }
    } else {
        payload
    };

    let kind = args.schedule.kind.trim().to_ascii_lowercase();
    let (schedule, due_at_ms, recurrence) = if kind == "recurring" || kind == "recurrence" {
        let rule = args
            .schedule
            .recurrence
            .as_deref()
            .map(str::trim)
            .filter(|rule| !rule.is_empty())
            .ok_or_else(|| {
                GeminiError::Decode("a recurring task needs a recurrence rule".to_string())
            })?;

        let parsed = Recurrence::parse(rule)
            .map_err(|err| GeminiError::Decode(format!("bad recurrence rule: {err}")))?;
        let due_at_ms = parsed.next_after_ms(now_ms).ok_or_else(|| {
            GeminiError::Decode("the recurrence rule has no next occurrence".to_string())
        })?;

        (ScheduleKind::Recurring, due_at_ms, Some(rule.to_string()))
    } else {
        let at = args
            .schedule
            .at
            .as_deref()
            .map(str::trim)
            .filter(|at| !at.is_empty())
            .ok_or_else(|| {
                GeminiError::Decode("a one-time task needs a date and time".to_string())
            })?;

        let datetime =
            parse_local_datetime(at).map_err(|err| GeminiError::Decode(format!("{err}")))?;
        let due_at_ms =
            local_datetime_to_ms(datetime).map_err(|err| GeminiError::Decode(format!("{err}")))?;
        (ScheduleKind::OneTime, due_at_ms, None)
    };

    Ok(PlannedTask {
        prompt,
        due_at_ms,
        recurrence,
        schedule,
        action,
        payload,
    })
}

fn create_task_tool() -> Tool {
    Tool {
        function_declarations: vec![FunctionDeclaration {
            name: "create_task",
            description:
                "Create exactly one scheduled desktop task from the user's spoken request.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "A short imperative restatement of the task, e.g. 'Open Spotify and play focus music'."
                    },
                    "schedule": {
                        "type": "object",
                        "description": "Either a single local date-time or an RFC 5545 recurrence rule.",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["one_time", "recurring"],
                                "description": "Use one_time for a single reminder or action; recurring for repeats."
                            },
                            "at": {
                                "type": "string",
                                "description": "For one_time only: local ISO 8601 date-time with no offset, e.g. 2026-11-08T09:30:00. Resolve 'tomorrow', 'in ten minutes' and 'at nine' against the supplied local clock."
                            },
                            "recurrence": {
                                "type": "string",
                                "description": "For recurring only: an RFC 5545 RRULE without DTSTART, e.g. FREQ=DAILY;BYHOUR=9;BYMINUTE=0 or FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=18;BYMINUTE=30. Supported parts: FREQ (HOURLY, DAILY, WEEKLY, MONTHLY), INTERVAL, BYDAY, BYHOUR, BYMINUTE, BYMONTHDAY."
                            }
                        },
                        "required": ["kind"]
                    },
                    "action": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["open_app", "open_url", "run_search", "remind", "type_text"],
                                "description": "open_app launches a Windows app; open_url opens a URL; run_search searches the web; remind shows a Windows toast; type_text types into the focused window."
                            },
                            "payload": {
                                "type": "string",
                                "description": "open_app: app name such as 'Spotify'. open_url: absolute http(s) URL. run_search: search query. remind: reminder text. type_text: exact text to type."
                            }
                        },
                        "required": ["kind", "payload"]
                    }
                },
                "required": ["prompt", "schedule", "action"]
            }),
        }],
    }
}

#[derive(Debug, Deserialize)]
struct RunActionArgs {
    kind: String,
    payload: String,
}

/// Upper bound on a generated payload: a runaway model must not be able to
/// paste an essay into the focused window.
const MAX_ACTION_PAYLOAD: usize = 2_000;

fn build_planned_action(
    args: RunActionArgs,
    planned: ActionKind,
    prompt: &str,
) -> Result<PlannedAction, GeminiError> {
    let action =
        ActionKind::parse(&args.kind).map_err(|err| GeminiError::Decode(format!("{err}")))?;

    let payload: String = args
        .payload
        .trim()
        .chars()
        .take(MAX_ACTION_PAYLOAD)
        .collect();
    let payload = if payload.is_empty() {
        match action {
            // A reminder is allowed to fall back to the task label.
            ActionKind::Remind => prompt.to_string(),
            _ => {
                return Err(GeminiError::Decode(format!(
                    "the {} action needs a payload",
                    action.as_str()
                )))
            }
        }
    } else {
        payload
    };

    if action != planned {
        return Err(GeminiError::Decode(format!(
            "Gemini changed the action from {} to {}",
            planned.as_str(),
            action.as_str()
        )));
    }

    Ok(PlannedAction { action, payload })
}

fn run_action_tool() -> Tool {
    Tool {
        function_declarations: vec![FunctionDeclaration {
            name: "run_action",
            description: "Run exactly one desktop action for a task that has come due.",
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["open_app", "open_url", "run_search", "remind", "type_text"],
                        "description": "Must be the kind the task was scheduled with; only the payload may be refined."
                    },
                    "payload": {
                        "type": "string",
                        "description": "open_app: the Windows app name. open_url: an absolute http(s) URL. run_search: a short search query. remind: the reminder text for the toast. type_text: the exact text to type, ready to paste."
                    }
                },
                "required": ["kind", "payload"]
            }),
        }],
    }
}

const SYSTEM_INSTRUCTION: &str = "You are Murmur, a terse desktop voice assistant. \
Reply with the result only: no preamble, no markdown fences, no explanation of what you did.";

/// Dictation instruction: clean up the speech, change nothing else.
const TRANSCRIBE_PROMPT: &str = "Transcribe the speech in this audio clip. \
Fix punctuation and capitalisation, fix obvious grammar slips, and remove filler words \
(um, uh, like, you know) and false starts. Keep the speaker's meaning, tone and wording. \
Keep the original language — never translate. Output only the final text: no quotes, \
no labels, no commentary.";

/// Circle-to-ask: the answer goes into a small notch card, so brevity is part
/// of the instruction, not a style preference.
const CIRCLED_INSTRUCTION: &str =
    "You answer short spoken questions about a circled region of the user's screen. \
Each request contains: a crop of the circled region, the full screen for context, and an audio \
question. Answer the question about the circled region concisely — one or two sentences, no \
preamble, no restating the question, no markdown headings. Use the full screen only to \
understand the surrounding context. Reply in the language the question was asked in, with plain \
text or at most a short bullet list.";

const SCHEDULE_INSTRUCTION: &str = "You turn one spoken scheduling request into one create_task tool call. \
Always call create_task exactly once and never answer with prose. \
Use the supplied current local date-time and timezone to resolve every relative phrase; never assume UTC. \
Schedule kinds: \
(1) one_time — set schedule.kind='one_time' and schedule.at to a local ISO 8601 date-time WITHOUT a timezone \
offset, e.g. 2026-11-08T09:30:00. If the requested time already passed today, choose the next sensible future \
occurrence. \
(2) recurring — set schedule.kind='recurring' and schedule.recurrence to an RFC 5545 RRULE without DTSTART, \
e.g. FREQ=DAILY;BYHOUR=9;BYMINUTE=0 or FREQ=WEEKLY;BYDAY=MO,WE,FR;BYHOUR=18;BYMINUTE=30. Supported parts are \
FREQ=HOURLY|DAILY|WEEKLY|MONTHLY, INTERVAL, BYDAY, BYHOUR, BYMINUTE and BYMONTHDAY. \
Actions: open_app for launching a named Windows app (payload is the app name), open_url for an absolute http(s) \
URL (payload is the URL), run_search for a web search (payload is the query), remind for a Windows toast \
(payload is the reminder text), type_text for typing into the focused window (payload is the exact text). \
When the user asks to be reminded and names no other action, use remind. \
Keep prompt short and imperative; it is the task list label. Include the action target in payload.";

const EXECUTE_INSTRUCTION: &str = "You run scheduled desktop tasks at the moment they come due. \
Call run_action exactly once and never answer with prose. \
The task was planned when the user spoke it, so keep the planned action kind and only refine its payload \
so it is concrete and correct now: \
open_app — the Windows app name; open_url — an absolute http(s) URL; run_search — a short, specific web-search \
query; remind — one or two sentences to show in a Windows toast; type_text — the final text the user asked for, \
ready to paste, with no commentary or quotation marks around it. \
Never change the action kind, never add extra steps, and never return an empty payload.";

fn map_transport_error(err: reqwest::Error) -> GeminiError {
    if err.is_timeout() {
        GeminiError::Timeout
    } else if err.is_connect() {
        GeminiError::NoNetwork
    } else {
        GeminiError::Rejected(err.to_string())
    }
}

fn classify(status: StatusCode, detail: &str) -> GeminiError {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return GeminiError::InvalidKey;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return GeminiError::RateLimited;
    }
    // Google answers a bad key with 400 API_KEY_INVALID, not 401.
    if status == StatusCode::BAD_REQUEST && detail.to_ascii_lowercase().contains("api key") {
        return GeminiError::InvalidKey;
    }
    if status.is_server_error() {
        return GeminiError::Server(detail.to_string());
    }
    GeminiError::Rejected(detail.to_string())
}

/// Pulls `error.message` out of Gemini's error envelope, with the raw body as
/// a fallback.
fn error_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct Envelope {
        error: Option<ApiError>,
    }

    #[derive(Deserialize)]
    struct ApiError {
        message: Option<String>,
        status: Option<String>,
    }

    serde_json::from_str::<Envelope>(body)
        .ok()
        .and_then(|envelope| envelope.error)
        .map(|error| match (error.message, error.status) {
            (Some(message), Some(status)) => format!("{message} ({status})"),
            (Some(message), None) => message,
            (None, Some(status)) => status,
            (None, None) => String::new(),
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| truncate(body, 200))
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(kind: &str, payload: &str) -> RunActionArgs {
        RunActionArgs {
            kind: kind.into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn planned_action_refines_the_payload_of_the_scheduled_kind() {
        let planned = build_planned_action(
            args("run_search", "  focus music  "),
            ActionKind::RunSearch,
            "play focus music",
        )
        .expect("valid plan");

        assert_eq!(planned.action, ActionKind::RunSearch);
        assert_eq!(planned.payload, "focus music");
    }

    #[test]
    fn planned_action_refuses_to_change_the_action() {
        let err = build_planned_action(args("type_text", "hello"), ActionKind::Remind, "stretch")
            .expect_err("a changed kind must be rejected");

        assert!(format!("{err}").contains("changed the action"));
    }

    #[test]
    fn planned_action_falls_back_to_the_prompt_for_a_reminder() {
        let planned = build_planned_action(args("remind", "   "), ActionKind::Remind, "stretch")
            .expect("valid plan");

        assert_eq!(planned.action, ActionKind::Remind);
        assert_eq!(planned.payload, "stretch");
    }

    #[test]
    fn planned_action_rejects_empty_payloads_and_bounds_long_ones() {
        assert!(build_planned_action(args("open_url", " "), ActionKind::OpenUrl, "x").is_err());

        let long = "a".repeat(MAX_ACTION_PAYLOAD + 50);
        let planned = build_planned_action(args("type_text", &long), ActionKind::TypeText, "x")
            .expect("valid plan");

        assert_eq!(planned.payload.chars().count(), MAX_ACTION_PAYLOAD);
    }
}
