use sandbox_common::{AuditEvent, Result, SandboxError};
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub trait AuditSink {
    fn write_event(&self, event: AuditEvent) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JsonlAuditSink {
    path: PathBuf,
}

impl JsonlAuditSink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl AuditSink for JsonlAuditSink {
    fn write_event(&self, event: AuditEvent) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            create_dir_all(parent).map_err(|error| {
                SandboxError::System(format!(
                    "failed to create audit directory '{}': {error}",
                    parent.display()
                ))
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                SandboxError::System(format!(
                    "failed to open audit log '{}': {error}",
                    self.path.display()
                ))
            })?;
        let line = serde_json::to_string(&event)?;
        writeln!(file, "{line}").map_err(|error| {
            SandboxError::System(format!(
                "failed to write audit log '{}': {error}",
                self.path.display()
            ))
        })?;

        Ok(())
    }
}
