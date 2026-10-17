//! macOS platform layer. Stub until the US1 platform tasks; every call reports `Unsupported`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::{Platform, PlatformError, Result};
use crate::procmon::snapshot::{Connection, Footprint, ProcessKey};

#[derive(Debug, Default)]
pub struct OsPlatform;

impl Platform for OsPlatform {
    fn cmdline(&mut self, _key: ProcessKey) -> Result<Arc<[Box<str>]>> {
        Err(PlatformError::Unsupported)
    }

    fn cwd(&mut self, _key: ProcessKey) -> Result<PathBuf> {
        Err(PlatformError::Unsupported)
    }

    fn footprint(&mut self, _key: ProcessKey) -> Result<Footprint> {
        Err(PlatformError::Unsupported)
    }

    fn tcp_table(&mut self, _owners: &HashMap<u32, ProcessKey>) -> Result<Vec<Connection>> {
        Err(PlatformError::Unsupported)
    }

    fn suspend(&mut self, _key: ProcessKey) -> Result<()> {
        Err(PlatformError::Unsupported)
    }

    fn resume(&mut self, _key: ProcessKey) -> Result<()> {
        Err(PlatformError::Unsupported)
    }

    fn kill(&mut self, _key: ProcessKey, _force: bool) -> Result<()> {
        Err(PlatformError::Unsupported)
    }
}
