//! UI-oriented data transfer objects exposed by the application facade.
//!
//! These are stable, serializable product concepts (Proyecto, Material,
//! Creación, Publicación), never internal infrastructure. IDs are present for
//! command addressing but are presentation-neutral; the frontend decides what
//! to render and must never interpret paths, hashes, or runtime state from
//! these values.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialView {
    pub id: String,
    pub display_name: String,
    pub original_file_name: String,
    /// Stable kind code derived from the file name: `pdf`, `image`, `document`,
    /// `spreadsheet`, `presentation`, `text`, or `other`.
    pub kind: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationView {
    pub id: String,
    pub display_name: String,
    /// `web`, `document`, `image`, or `file`.
    pub kind: String,
    /// `public` ("Se compartirá") or `private` ("Privado").
    pub visibility: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationView {
    /// `local` or `published`.
    pub state: String,
    /// Public URL when `state` is `published`; always runtime-only and never
    /// persisted.
    pub public_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub materials: Vec<MaterialView>,
    pub creations: Vec<CreationView>,
    pub publication: PublicationView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunView {
    /// `completed`, `failed`, or `cancelled`.
    pub status: String,
    pub registered_creation_ids: Vec<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatusView {
    pub version: String,
    /// `stopped`, `starting`, `ready`, or `failed`.
    pub agent: String,
}
