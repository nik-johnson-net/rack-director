use axum::{body::Body, response::IntoResponse};

pub enum Error {
    BadRequest(String),
    ServerError(anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> axum::response::Response {
        match self {
            Error::BadRequest(reason) => axum::response::Response::builder()
                .status(400)
                .body(Body::from(reason))
                .expect("building body"),
            Error::ServerError(error) => {
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
        Self::ServerError(value)
    }
}
