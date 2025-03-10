use anyhow::Result;
use candle_core::Device;
use glowrs::core::utils::parse_repo_string;
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::infer::embed::EmbeddingsClient;
use crate::server::infer::embed::EmbeddingsHandler;
use crate::server::infer::DedicatedExecutor;

// TODO: Create a struct to hold the core map
// TODO: Needs to support externally provided models (e.g. other gRPC services)
type EmbeddingModelMap =
    HashMap<String, (EmbeddingsClient, Arc<DedicatedExecutor<EmbeddingsHandler>>)>;

/// Represents the state of the server.
#[derive(Clone)]
pub struct ServerState {
    pub default_model: String,
    pub model_map: EmbeddingModelMap,
}

impl ServerState {
    pub fn new(model_repos: Vec<String>, device: &Device) -> Result<Self> {
        if model_repos.is_empty() {
            return Err(anyhow::anyhow!("No models provided"));
        }

        let map = model_repos
            .iter()
            .filter_map(|model_repo| {
                let (name, _) = parse_repo_string(&model_repo).ok()?;
                let handler = EmbeddingsHandler::from_repo_string(&model_repo, device).ok()?;
                let executor = DedicatedExecutor::new(handler).ok()?;
                let client = EmbeddingsClient::new(&executor);

                Some((name.to_string(), (client, Arc::new(executor))))
            })
            .collect::<EmbeddingModelMap>();

        Ok(Self { default_model: model_repos[0].clone(), model_map: map })
    }
}
