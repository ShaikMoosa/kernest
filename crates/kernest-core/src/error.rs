use std::path::PathBuf;

/// Errors produced while reading, writing or editing a font.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid Kernest JSON: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("{0} was created with a newer version of Kernest (format version {1}); this version reads up to {2}")]
    NewerFormat(PathBuf, u32, u32),

    #[error("invalid font: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) trait IoContext<T> {
    fn at(self, path: &std::path::Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: &std::path::Path) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}
