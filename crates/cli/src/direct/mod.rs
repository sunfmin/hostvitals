//! Native observations and explicit fixture substitution share the same typed result.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fs, path::PathBuf};

pub mod arguments;
pub mod host;
pub mod ioreport;
pub mod libproc;
pub mod responsibility;
pub mod rusage;

/// An unavailable read retains its cause without inventing a measurement.
#[derive(Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reading<T> {
    Available { data: T },
    Unavailable { reason: String },
}

/// Where native reads come from during this invocation; fixtures never silently fall back to the host.
pub struct Source(pub Option<PathBuf>);
impl Source {
    /// Runs one native observation or reads its entire result from a named JSON fixture.
    pub fn read<T: DeserializeOwned>(
        &self,
        name: &str,
        native: impl FnOnce() -> Reading<T>,
    ) -> Reading<T> {
        match &self.0 {
            None => native(),
            Some(dir) => match fs::read(dir.join(format!("{name}.json")))
                .map_err(|e| e.to_string())
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
            {
                Ok(reading) => reading,
                Err(error) => Reading::Unavailable {
                    reason: format!("fixture {name}: {error}"),
                },
            },
        }
    }
}
mod dylib;
impl<T> Reading<T> {
    /// Preserves the native error so doctor can report every interface independently.
    fn from_result(result: Result<T, String>) -> Self {
        match result {
            Ok(data) => Self::Available { data },
            Err(reason) => Self::Unavailable { reason },
        }
    }
}
