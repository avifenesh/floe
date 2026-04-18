use std::path::{Path, PathBuf};
use std::sync::Arc;

use adr_core::{artifact::PrRef, Artifact};
use anyhow::{Context, Result};

use crate::cache::Cache;
use crate::job::{Job, JobStatus, ProgressEvent};

pub async fn run_pipeline(job: Arc<Job>, base: PathBuf, head: PathBuf, cache: Arc<Cache>) {
    match run_inner(&job, &base, &head, &cache).await {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = job.progress.send(ProgressEvent {
                stage: "error".into(),
                percent: 100,
                message: msg.clone(),
            });
            *job.status.write().await = JobStatus::Error { message: msg };
        }
    }
}

async fn run_inner(job: &Arc<Job>, base: &Path, head: &Path, cache: &Cache) -> Result<()> {
    let key = cache.key(base, head);

    // Cache hit → skip the pipeline, publish a single "ready" event.
    if let Some(a) = cache.get(&key)? {
        *job.artifact.write().await = Some(a);
        *job.status.write().await = JobStatus::Ready;
        let _ = job.progress.send(ProgressEvent {
            stage: "ready".into(),
            percent: 100,
            message: "cached".into(),
        });
        return Ok(());
    }

    emit(job, "parse-base", 10, "walking base tree").await;
    let base_graph = {
        let base = base.to_path_buf();
        tokio::task::spawn_blocking(move || adr_parse::Ingest::new("base").ingest_dir(&base))
            .await
            .context("parse-base join")??
    };

    emit(job, "parse-head", 30, "walking head tree").await;
    let head_graph = {
        let head = head.to_path_buf();
        tokio::task::spawn_blocking(move || adr_parse::Ingest::new("head").ingest_dir(&head))
            .await
            .context("parse-head join")??
    };

    emit(job, "cfg", 55, "building control-flow graphs").await;
    let base_cfg = {
        let g = base_graph.clone();
        let root = base.to_path_buf();
        tokio::task::spawn_blocking(move || adr_cfg::build_for_graph(&g, &root))
            .await
            .context("cfg-base join")??
    };
    let head_cfg = {
        let g = head_graph.clone();
        let root = head.to_path_buf();
        tokio::task::spawn_blocking(move || adr_cfg::build_for_graph(&g, &root))
            .await
            .context("cfg-head join")??
    };

    emit(job, "hunks", 75, "extracting semantic hunks").await;
    let hunks = adr_hunks::extract_all(&base_graph, &head_graph);

    let mut artifact = Artifact::new(PrRef {
        repo: "unknown".into(),
        base_sha: base.display().to_string(),
        head_sha: head.display().to_string(),
    });
    artifact.base = base_graph;
    artifact.head = head_graph;
    artifact.base_cfg = base_cfg;
    artifact.head_cfg = head_cfg;
    artifact.hunks = hunks;

    emit(job, "flows", 90, "structural flow clustering").await;
    artifact.flows = adr_flows::cluster(&artifact);

    cache.put(&key, &artifact)?;
    *job.artifact.write().await = Some(artifact);
    *job.status.write().await = JobStatus::Ready;

    let _ = job.progress.send(ProgressEvent {
        stage: "ready".into(),
        percent: 100,
        message: "done".into(),
    });
    Ok(())
}

async fn emit(job: &Arc<Job>, stage: &str, percent: u8, message: &str) {
    let _ = job.progress.send(ProgressEvent {
        stage: stage.into(),
        percent,
        message: message.into(),
    });
}
