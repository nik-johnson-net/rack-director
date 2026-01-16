use super::super::{AppState, error::Error as HttpError};
use crate::dhcp::validation;
use crate::dhcp::{DhcpNetwork, DhcpPool, Lease, StaticReservation};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        // Networks
        .route("/ui/dhcp/networks", get(list_networks).post(create_network))
        .route(
            "/ui/dhcp/networks/{id}",
            get(get_network).put(update_network).delete(delete_network),
        )
        // Pools
        .route(
            "/ui/dhcp/networks/{network_id}/pools",
            get(list_pools).post(create_pool),
        )
        .route("/ui/dhcp/pools/{id}", put(update_pool).delete(delete_pool))
        // Static Reservations
        .route(
            "/ui/dhcp/networks/{network_id}/static-reservations",
            get(list_static_reservations).post(create_static_reservation),
        )
        .route(
            "/ui/dhcp/static-reservations/{id}",
            delete(delete_static_reservation),
        )
        // Leases
        .route(
            "/ui/dhcp/networks/{network_id}/leases",
            get(list_leases_by_network),
        )
        .route("/ui/dhcp/leases/{id}/make-static", post(make_lease_static))
        .with_state(state)
}

// ========== Request/Response Types ==========

#[derive(Debug, Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub lease_duration: u32,
    pub relay_agent_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNetworkRequest {
    pub name: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Option<Vec<String>>,
    pub lease_duration: Option<u32>,
    pub relay_agent_address: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    pub range_start: String,
    pub range_end: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePoolRequest {
    pub name: Option<String>,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStaticReservationRequest {
    pub mac_address: String,
    pub ip_address: String,
    pub hostname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MakeStaticRequest {
    pub ip_address: Option<String>,
    pub hostname: Option<String>,
}

// ========== Network Handlers ==========

/// List all DHCP networks
async fn list_networks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DhcpNetwork>>, HttpError> {
    let networks = state.dhcp_store.list_networks().await?;
    Ok(Json(networks))
}

/// Get a specific network by ID
async fn get_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DhcpNetwork>, HttpError> {
    let network = state.dhcp_store.get_network(id).await?;
    Ok(Json(network))
}

/// Create a new DHCP network
async fn create_network(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateNetworkRequest>,
) -> Result<(StatusCode, Json<DhcpNetwork>), HttpError> {
    let network = state
        .dhcp_store
        .create_network(
            &req.name,
            &req.subnet,
            &req.gateway,
            &req.dns_servers,
            req.lease_duration,
            req.relay_agent_address.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(network)))
}

/// Update an existing network
async fn update_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNetworkRequest>,
) -> Result<Json<DhcpNetwork>, HttpError> {
    let network = state
        .dhcp_store
        .update_network(
            id,
            req.name.as_deref(),
            req.subnet.as_deref(),
            req.gateway.as_deref(),
            req.dns_servers.as_deref(),
            req.lease_duration,
            req.relay_agent_address.as_ref().map(|opt| opt.as_deref()),
        )
        .await?;

    Ok(Json(network))
}

/// Delete a network
async fn delete_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_network(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Pool Handlers ==========

/// List all pools for a network
async fn list_pools(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<DhcpPool>>, HttpError> {
    let pools = state.dhcp_store.list_pools_for_network(network_id).await?;
    Ok(Json(pools))
}

/// Create a new pool in a network
async fn create_pool(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<DhcpPool>), HttpError> {
    let pool = state
        .dhcp_store
        .create_pool(network_id, &req.name, &req.range_start, &req.range_end)
        .await?;

    Ok((StatusCode::CREATED, Json(pool)))
}

/// Update an existing pool
async fn update_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdatePoolRequest>,
) -> Result<Json<DhcpPool>, HttpError> {
    let pool = state
        .dhcp_store
        .update_pool(
            id,
            req.name.as_deref(),
            req.range_start.as_deref(),
            req.range_end.as_deref(),
        )
        .await?;

    Ok(Json(pool))
}

/// Delete a pool
async fn delete_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_pool(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Static Reservation Handlers ==========

/// List all static reservations for a network
async fn list_static_reservations(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<StaticReservation>>, HttpError> {
    let reservations = state
        .dhcp_store
        .list_static_reservations(network_id)
        .await?;
    Ok(Json(reservations))
}

/// Create a new static reservation
async fn create_static_reservation(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
    Json(req): Json<CreateStaticReservationRequest>,
) -> Result<(StatusCode, Json<StaticReservation>), HttpError> {
    // Fetch the network to get the subnet
    let network = state.dhcp_store.get_network(network_id).await?;

    // Validate the IP is within the subnet
    validation::validate_ip_in_network(&req.ip_address, &network.subnet)
        .map_err(|e| HttpError::BadRequest(e.to_string()))?;

    let reservation = state
        .dhcp_store
        .create_static_reservation(
            network_id,
            &req.mac_address,
            &req.ip_address,
            req.hostname.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(reservation)))
}

/// Delete a static reservation
async fn delete_static_reservation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_static_reservation(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Lease Handlers ==========

/// List all leases for a specific network
async fn list_leases_by_network(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<Lease>>, HttpError> {
    let leases = state.dhcp_store.get_leases_by_network(network_id).await?;
    Ok(Json(leases))
}

/// Convert a dynamic lease to a static reservation
async fn make_lease_static(
    State(state): State<Arc<AppState>>,
    Path(lease_id): Path<i64>,
    Json(req): Json<MakeStaticRequest>,
) -> Result<(StatusCode, Json<StaticReservation>), HttpError> {
    // Get the lease by ID
    let lease = state
        .dhcp_store
        .get_lease_by_id(lease_id)
        .await?
        .ok_or_else(|| HttpError::NotFound(format!("Lease {} not found", lease_id)))?;

    // Verify the lease has a network_id
    let network_id = lease
        .network_id
        .ok_or_else(|| HttpError::BadRequest("Lease has no associated network".to_string()))?;

    // Fetch the network to get the subnet
    let network = state.dhcp_store.get_network(network_id).await?;

    // Determine the IP address to use (from request or lease)
    let ip_address = req.ip_address.as_deref().unwrap_or(&lease.ip_address);

    // Validate IP is in subnet
    validation::validate_ip_in_network(ip_address, &network.subnet)
        .map_err(|e| HttpError::BadRequest(e.to_string()))?;

    // Use hostname from request if provided, otherwise use lease hostname
    let hostname = req.hostname.or(lease.hostname);

    // Create static reservation
    let reservation = state
        .dhcp_store
        .create_static_reservation(
            network_id,
            &lease.mac_address,
            ip_address,
            hostname.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(reservation)))
}
