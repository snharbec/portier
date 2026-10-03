//! The built SvelteKit app, embedded in the binary. Unknown paths fall back to the SPA shell.

use axum::{
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/build"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let (file, name) = match Assets::get(path) {
        Some(file) if !path.is_empty() => (Some(file), path),
        _ => (Assets::get("index.html"), "index.html"),
    };
    match file {
        Some(file) => {
            let mime = mime_guess::from_path(name).first_or_octet_stream();
            // Hashed build output never changes; the shell must always be fresh.
            let cache = if name.starts_with("_app/immutable/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref().to_string()),
                    (header::CACHE_CONTROL, cache.to_string()),
                ],
                file.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "frontend not built: run `npm run build` in web/").into_response(),
    }
}
