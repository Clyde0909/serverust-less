//! Queue API endpoints

use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use std::sync::Arc;

use crate::api::AppState;
use crate::error::AppError;
use crate::models::QueueStatusResponse;

/// Create the queue router
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/queue/status", get(get_queue_status))
}

/// Get queue status
#[utoipa::path(
    get,
    path = "/api/v1/queue/status",
    tag = "queue",
    responses(
        (status = 200, description = "Queue status", body = QueueStatusResponse)
    )
)]
pub async fn get_queue_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<QueueStatusResponse>, AppError> {
    // Get in-memory queue size from worker pool if available
    let in_memory_size = 0; // TODO: Get from worker pool
    let response = state.queue_service.get_status(in_memory_size).await?;
    Ok(Json(response))
}
