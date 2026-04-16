//! Shared daemon state: cache, parser, and runtime configuration.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::cache::ProposalCache;
use crate::parser::LanguageParsers;

/// Shared state for the daemon, accessible from RPC handlers.
pub struct DaemonState {
    /// The proposal cache database.
    pub cache: ProposalCache,
    /// Tree-sitter parser registry for supported languages.
    pub parser: RwLock<LanguageParsers>,
    /// Runtime directory for sockets, etc.
    pub runtime_dir: PathBuf,
}

impl DaemonState {
    /// Create a new daemon state.
    pub fn new(runtime_dir: PathBuf) -> anyhow::Result<Arc<Self>> {
        let cache_path = runtime_dir.join("proposals.db");
        let cache = ProposalCache::open(&cache_path)?;
        let parser = LanguageParsers::new()?;

        tracing::info!(
            db_path = %cache_path.display(),
            "proposal cache opened"
        );

        Ok(Arc::new(Self {
            cache,
            parser: RwLock::new(parser),
            runtime_dir,
        }))
    }
}
