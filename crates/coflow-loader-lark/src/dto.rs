use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(crate) struct AuthResponse {
    pub(crate) code: i64,
    pub(crate) msg: Option<String>,
    pub(crate) tenant_access_token: Option<String>,
    /// Server-declared TTL in seconds. Lark documents 7200 today; callers
    /// nonetheless treat this as advisory and apply a safety margin before
    /// reuse.
    #[serde(default)]
    pub(crate) expire: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiEnvelope<T> {
    pub(crate) code: i64,
    pub(crate) msg: Option<String>,
    pub(crate) data: Option<T>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WikiNodeData {
    pub(crate) node: WikiNode,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WikiNode {
    pub(crate) obj_type: String,
    pub(crate) obj_token: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SheetsQueryData {
    pub(crate) sheets: Vec<LarkSheetMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LarkSheetMetadata {
    pub(crate) sheet_id: String,
    pub(crate) title: String,
    #[serde(default, flatten)]
    grid: GridContainer,
}

impl LarkSheetMetadata {
    pub(crate) fn row_count(&self) -> usize {
        self.grid
            .grid_properties
            .as_ref()
            .map_or(0, |grid| grid.row_count)
    }

    pub(crate) fn column_count(&self) -> usize {
        self.grid
            .grid_properties
            .as_ref()
            .map_or(0, |grid| grid.column_count)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct GridContainer {
    grid_properties: Option<GridProperties>,
}

#[derive(Debug, Clone, Deserialize)]
struct GridProperties {
    #[serde(default)]
    row_count: usize,
    #[serde(default)]
    column_count: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ValuesData {
    #[serde(rename = "valueRange", alias = "value_range")]
    pub(crate) value_range: ValueRange,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ValueRange {
    #[serde(default)]
    pub(crate) values: Vec<Vec<Value>>,
}
