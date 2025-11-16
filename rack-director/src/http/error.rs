use axum::{body::Body, response::IntoResponse};

pub enum Error {
    BadRequest(String),
    NotFound(String),
    #[allow(clippy::enum_variant_names)] // ServerInternalError is the HTTP response code name
    ServerInternalError(anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
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
