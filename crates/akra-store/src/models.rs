use akra_core::ingress::ActivityKind;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ActivitySummary {
    pub id: i64,
    pub provider: String,
    pub activity_kind: ActivityKind,
    pub prompt: String,
    pub project: Option<ActivityProjectSummary>,
    pub time: ActivityTimeSummary,
    pub conversation_index: i64,
    pub conversation_total: i64,
    pub result_summary_status: ResultSummaryStatus,
    pub prompt_summary: ActivityPromptSummary,
}

#[derive(Debug, Serialize)]
pub struct ActivityProjectSummary {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ActivityTimeSummary {
    pub value: Option<String>,
    pub provenance: ActivityTimeProvenance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityTimeProvenance {
    Captured,
    LegacyRecorded,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct ActivityDetail {
    pub id: i64,
    pub provider: String,
    pub activity_kind: ActivityKind,
    pub prompt: String,
    pub project: Option<ActivityProjectSummary>,
    pub captured_at: ActivityTimeSummary,
    pub first_recorded_at: ActivityTimeSummary,
    pub on_canvas: bool,
    pub submitted_cwd: Option<String>,
    pub origin: ActivityOriginDetail,
    pub technical: ActivityTechnicalDetail,
    pub result_summary: ActivityResultSummary,
    pub prompt_summary: ActivityPromptSummary,
    pub selected_turn: ActivityConversationTurn,
    pub conversation: Vec<ActivityConversationTurn>,
    pub conversation_index: i64,
    pub conversation_total: i64,
    pub conversation_has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct ActivityOriginDetail {
    pub id: i64,
    pub kind: String,
    pub resolution_source: String,
    pub display_path: String,
    pub activity_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ActivityTechnicalDetail {
    pub session_id: String,
    pub turn_id: String,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActivityConversationTurn {
    pub id: i64,
    pub activity_kind: ActivityKind,
    pub prompt: String,
    pub project: Option<ActivityProjectSummary>,
    pub time: ActivityTimeSummary,
    pub on_canvas: bool,
    pub selected: bool,
    pub result_summary: ActivityResultSummary,
    pub prompt_summary: ActivityPromptSummary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSummaryStatus {
    Pending,
    Ready,
    Unavailable,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActivityResultSummary {
    pub status: ResultSummaryStatus,
    pub lines: Option<[String; 3]>,
}

impl ActivityResultSummary {
    pub const fn unavailable() -> Self {
        Self {
            status: ResultSummaryStatus::Unavailable,
            lines: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSummaryStatus {
    Pending,
    Ready,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSummaryMode {
    Contextual,
    Standalone,
    Passthrough,
    Fallback,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActivityPromptSummary {
    pub status: PromptSummaryStatus,
    pub mode: PromptSummaryMode,
    pub text: Option<String>,
}

impl ActivityPromptSummary {
    pub const fn unavailable() -> Self {
        Self {
            status: PromptSummaryStatus::Unavailable,
            mode: PromptSummaryMode::Fallback,
            text: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CanvasNodeSummary {
    pub id: i64,
    pub activity_event_id: i64,
    pub position_x: f64,
    pub position_y: f64,
}

#[derive(Debug, Serialize)]
pub struct CanvasEdgeSummary {
    pub id: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ProviderIntegration {
    pub provider: String,
    pub enabled: bool,
    pub prompt_summary_mode: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub origin_count: i64,
    pub activity_count: i64,
    pub needs_setup: bool,
    pub latest_activity_at_us: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct OriginSummary {
    pub id: i64,
    pub display_path: String,
    pub kind: String,
    pub resolution_source: String,
    pub setup_state: String,
    pub routing_mode: String,
    pub default_project_id: Option<i64>,
    pub default_project_name: Option<String>,
    pub activity_count: i64,
    pub conversation_count: i64,
    pub recommended_mode: String,
}
