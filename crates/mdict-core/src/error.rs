use std::fmt;

/// Error type shared by the whole engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("文件格式错误: {0}")]
    Format(String),
    #[error("不支持的特性: {0}")]
    Unsupported(String),
    #[error("索引错误: {0}")]
    Index(String),
    #[error("未找到: {0}")]
    NotFound(String),
    #[error("操作已取消")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn format(msg: impl fmt::Display) -> Self {
        Self::Format(msg.to_string())
    }

    pub fn unsupported(msg: impl fmt::Display) -> Self {
        Self::Unsupported(msg.to_string())
    }

    pub fn index(msg: impl fmt::Display) -> Self {
        Self::Index(msg.to_string())
    }

    pub fn other(msg: impl fmt::Display) -> Self {
        Self::Other(msg.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
