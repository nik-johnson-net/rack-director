use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
};

use crate::http::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/dhcp/leases", get(get_all_dhcp_leases))
        .route("/ui/dhcp/leases/{mac}", get(get_dhcp_lease_by_mac))
        .with_state(state)
}

async fn get_all_dhcp_leases(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::dhcp::Lease>>, StatusCode> {
    match state.dhcp_store.get_all_leases().await {
        Ok(leases) => Ok(Json(leases)),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_dhcp_lease_by_mac(
    State(state): State<Arc<AppState>>,
    Path(mac): Path<String>,
) -> Result<Json<crate::dhcp::Lease>, StatusCode> {
    match state.dhcp_store.get_lease_by_mac(&mac).await {
        Ok(Some(lease)) => Ok(Json(lease)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
