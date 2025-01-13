//  WWW.rs
//    by Lut99
//
//  Created:
//    17 Jul 2024, 18:59:49
//  Last edited:
//    13 Jan 2025, 23:03:00
//  Auto updated?
//    Yes
//
//  Description:
//!   Provides an axum path for hosting static files in some folder.
//

use std::ffi::OsStr;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::{self, ConnectInfo, State};
use axum::http::HeaderValue;
use axum_extra::body::AsyncReadBody;
use error_trace::trace;
use hyper::{HeaderMap, StatusCode, header};
use log::{debug, error, info};
use tokio::fs::File;

use crate::state::Context;


/***** HELPER FUNCTIONS *****/
/// Streams the given file back to the user.
///
/// # Arguments
/// - `state`: A shared [`Context`] that situates this path.
/// - `client`: An [`SocketAddr`] that tells us the address of the connected client.
/// - `code`: The code to return when the streaming is a success (so far).
/// - `path`: The full path of the file to stream back.
///
/// # Returns
/// Either:
/// - 200 OK with the found file if the the user had access;
/// - 501 INTERNAL SERVER ERROR if something went wrong while streaming the file.
async fn return_file(state: &Arc<Context>, client: SocketAddr, code: StatusCode, path: impl AsRef<Path>) -> (StatusCode, HeaderMap, AsyncReadBody) {
    let path: &Path = path.as_ref();
    debug!(target: client.to_string().as_str(), "Returning file '{}' with {} {} to user", path.display(), code.as_u16(), code.canonical_reason().unwrap_or("???"));

    // Attempt to open the file
    let handle: File = match File::open(path).await {
        Ok(handle) => handle,
        Err(err) => {
            error!(target: client.to_string().as_str(), "{}", trace!(("Failed to open file '{}'", path.display()), err));
            return (code, HeaderMap::new(), AsyncReadBody::new(b"Internal server error".as_slice()));
        },
    };

    // Guess the file's mime type
    let mime_type: HeaderValue = match path.extension().and_then(OsStr::to_str) {
        Some("html") => HeaderValue::from_static("text/html"),
        Some("js") => HeaderValue::from_static("text/javascript"),
        Some("css") => HeaderValue::from_static("text/css"),
        _ => HeaderValue::from_static("text/plain"),
    };

    // Get the file's metadata (length, to be precise)
    let len: u64 = match handle.metadata().await {
        Ok(md) => md.len(),
        Err(err) => {
            error!(target: client.to_string().as_str(), "{}", trace!(("Failed to read metadata of file '{}'", path.display()), err));
            return (code, HeaderMap::new(), AsyncReadBody::new(b"Internal server error".as_slice()));
        },
    };

    // Create the header map
    let mut headers: HeaderMap = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, mime_type);
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    headers.insert(header::SERVER, HeaderValue::from_str(&format!("{}/{}", state.name, state.version)).unwrap());

    // Stream it as the body
    let body: AsyncReadBody = AsyncReadBody::new(handle);
    (code, headers, body)
}





/***** LIBRARY *****/
/// Fetches files according to the given path.
///
/// This respects the user-provided [`SiteSecurity`](crate::state::SiteSecurity)-file, which tells us what kind of security requirements each file has.
///
/// # Arguments
/// - `state`: A shared [`Context`] that situates this path.
/// - `client`: An [`SocketAddr`] that tells us the address of the connected client.
/// - `path`: The path of the file that was matched.
///
/// # Returns
/// Either:
/// - 200 OK with the found file if the the user had access; or
/// - 404 NOT FOUND with the not-found-page if the file was not found.
///
/// # Errors
/// This function errors if it found but failed to load a file.
#[cfg_attr(feature = "axum-debug", axum_macros::debug_handler)]
pub async fn handle(
    State(state): State<Arc<Context>>,
    ConnectInfo(client): ConnectInfo<SocketAddr>,
    path: Option<extract::Path<PathBuf>>,
) -> (StatusCode, HeaderMap, AsyncReadBody) {
    let path: PathBuf = path.map(|p| p.0).unwrap_or_default();
    info!(target: client.to_string().as_str(), "Handling GET {:?}", path.display());

    // First, get the full file path
    let mut file_path: PathBuf = state.site.clone();
    file_path.extend(path.components().skip_while(|c| matches!(c, Component::RootDir)));

    // Canonicalize it
    let mut file_path: PathBuf = match file_path.canonicalize() {
        // If found, then ensure it didn't escape
        Ok(path) => {
            if path.starts_with(&state.site) {
                path
            } else {
                debug!(target: client.to_string().as_str(), "[404] Target file path '{}' escaped site directory", file_path.display());
                return return_file(&state, client, StatusCode::NOT_FOUND, &state.not_found_file).await;
            }
        },
        Err(err) => {
            debug!(target: client.to_string().as_str(), "{}", trace!(("[404] Target file path '{}' cannot be canonicalized", file_path.display()), err));
            return return_file(&state, client, StatusCode::NOT_FOUND, &state.not_found_file).await;
        },
    };
    // If it's a directory, then append `index.html`
    if file_path.is_dir() {
        file_path.push("index.html");
        if !file_path.exists() {
            debug!(target: client.to_string().as_str(), "[404] Target file path '{}' not found", file_path.display());
            return return_file(&state, client, StatusCode::NOT_FOUND, &state.not_found_file).await;
        }
    }
    debug!(target: client.to_string().as_str(), "Target file path: {}", file_path.display());

    // OK, return the file!
    return_file(&state, client, StatusCode::OK, file_path).await
}
