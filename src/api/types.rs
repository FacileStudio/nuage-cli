use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFile {
    pub id: i64,
    pub name: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub space_id: Option<i64>,
    pub mime_type: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFolder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub space_id: Option<i64>,
    pub updated_at: String,
}

/// A space the signed-in user belongs to.
///
/// `role` is the caller's role in it, not the space's own attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSpace {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSpaceRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSpaceRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteSpaceResponse {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStateResponse {
    pub files: Vec<ApiFile>,
    pub folders: Vec<ApiFolder>,
    pub server_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncChangesResponse {
    pub files: FileChanges,
    pub folders: FolderChanges,
    pub server_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanges {
    pub changed: Vec<ApiFile>,
    pub deleted: Vec<DeletedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderChanges {
    pub changed: Vec<ApiFolder>,
    pub deleted: Vec<DeletedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedItem {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub space_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldersListResponse {
    pub folders: Vec<ApiFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderDetailResponse {
    pub folder: ApiFolder,
    pub files: Vec<ApiFile>,
    pub folders: Vec<ApiFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResponse {
    pub id: i64,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<i64>,
    pub permission: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharesListResponse {
    pub shares: Vec<ShareResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokensListResponse {
    pub tokens: Vec<ApiToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub app: String,
    pub kind: String,
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub daily_quota: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_today: Option<i64>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyRequest {
    pub app: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_quota: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyResponse {
    pub key: ApiKey,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeysResponse {
    pub keys: Vec<ApiKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchApiResponse {
    pub results: Vec<SearchResultItem>,
    pub total: i32,
}

/// Response of `POST /files/upload/init`.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadSession {
    pub session_id: String,
}

/// Response of `POST /files/upload/{sessionId}/complete`, which nests the
/// created file under a `file` key.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadCompleteResponse {
    pub file: ApiFile,
}
