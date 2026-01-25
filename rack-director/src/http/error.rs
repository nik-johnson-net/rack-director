use std::collections::HashMap;
use std::net::AddrParseError;

use axum::{Json, body::Body, http::StatusCode, response::IntoResponse};
use common::Ipv4SubnetError;
use serde::Serialize;

pub enum Error {
    BadRequest(String),
    #[allow(clippy::enum_variant_names)]
    ValidationError(HashMap<String, String>),
    NotFound(String),
    #[allow(clippy::enum_variant_names)] // ServerInternalError is the HTTP response code name
    ServerInternalError(anyhow::Error),
}

#[derive(Serialize)]
struct ValidationErrorResponse {
    errors: HashMap<String, String>,
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Error::ValidationError(errors) => {
                let body = ValidationErrorResponse { errors };
                (StatusCode::BAD_REQUEST, Json(body)).into_response()
            }
            Error::BadRequest(reason) => axum::response::Response::builder()
                .status(400)
                .body(Body::from(reason))
                .expect("building body"),
            Error::NotFound(reason) => axum::response::Response::builder()
                .status(404)
                .body(Body::from(reason))
                .expect("building body"),
            Error::ServerInternalError(error) => {
                log::error!("Error: {:#}", error);
                axum::response::Response::builder()
                    .status(500)
                    .body(Body::empty())
                    .expect("building body")
            }
        }
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Self::ServerInternalError(value)
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::ServerInternalError(value.into())
    }
}

impl From<Ipv4SubnetError> for Error {
    fn from(value: Ipv4SubnetError) -> Self {
        Self::ServerInternalError(value.into())
    }
}

impl From<AddrParseError> for Error {
    fn from(value: AddrParseError) -> Self {
        Self::ServerInternalError(value.into())
    }
}
