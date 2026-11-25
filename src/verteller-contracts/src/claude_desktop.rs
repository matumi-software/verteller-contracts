//! The wire types of the Claude Desktop connection.
//!
//! The connection itself (the `verteller` entry in Claude Desktop's config file) is part of the
//! Pro build, which serves `GET`, `PUT` and `DELETE /api/v1/integrations/claude-desktop`. These
//! types are here so the UI's generated types come from one place and both builds agree on the body.

use serde::Serialize;

/// `connected`, `notConnected` or `unsupported`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "IntegrationState"))]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// The `verteller` entry exists and points at this binary.
    Connected,
    /// No entry, or one that points somewhere else.
    NotConnected,
    /// No Claude Desktop config folder here.
    Unsupported,
}

/// The body of `GET /api/v1/integrations/claude-desktop` and of `integrations.changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "IntegrationStatus"))]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Whether the entry is there.
    pub state: State,
    /// The config file, when the state is not `unsupported`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub config_path: Option<String>,
}
