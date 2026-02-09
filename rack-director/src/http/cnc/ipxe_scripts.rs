use axum::{
    http::{StatusCode, header},
    response::Response,
};

/// Generates an iPXE script that redirects to the main iPXE endpoint with UUID and MAC.
///
/// This script is sent to devices that boot without providing their UUID. It instructs
/// iPXE to chain-load back to the iPXE endpoint, this time including the device's UUID
/// and MAC address as query parameters.
///
/// # Arguments
/// * `root_url` - The base HTTP URL of the rack-director server
pub fn generate_uuid_script(root_url: &str) -> String {
    format!(
        r#"#!ipxe
# Chain boot to send uuid and mac
chain {root_url}/cnc/ipxe?uuid=${{uuid}}&mac=${{netX/mac}}
"#
    )
}

/// Generates an iPXE redirect response with UUID collection script.
///
/// This is a convenience wrapper around `generate_uuid_script` that returns a complete
/// HTTP response ready to be sent to the client.
///
/// # Arguments
/// * `root_url` - The base HTTP URL of the rack-director server
pub fn generate_uuid_redirect(root_url: &str) -> Response<String> {
    build_response(generate_uuid_script(root_url))
}

/// Builds an HTTP response containing an iPXE script.
///
/// Creates a 200 OK response with Content-Type: text/plain containing the provided
/// iPXE script. iPXE expects plain text responses.
///
/// # Arguments
/// * `script` - The complete iPXE script content
pub fn build_response(script: String) -> Response<String> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(script)
        .expect("response building should never error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_uuid_script() {
        let script = generate_uuid_script("http://example.com");
        assert!(script.contains("#!ipxe"));
        assert!(script.contains("chain http://example.com/cnc/ipxe?uuid=${uuid}&mac=${netX/mac}"));
    }

    #[test]
    fn test_generate_uuid_redirect() {
        let response = generate_uuid_redirect("http://example.com");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        let body = response.into_body();
        assert!(body.contains("chain http://example.com/cnc/ipxe?uuid=${uuid}&mac=${netX/mac}"));
    }

    #[test]
    fn test_build_response() {
        let script = "#!ipxe\nboot\n".to_string();
        let response = build_response(script.clone());
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
        assert_eq!(response.into_body(), script);
    }
}
