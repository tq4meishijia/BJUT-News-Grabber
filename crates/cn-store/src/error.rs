//! Store 层错误。

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Message(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Net(#[from] cn_net::NetError),
}

impl StoreError {
    pub fn msg(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}
