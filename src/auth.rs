//  AUTH.rs
//    by Lut99
//
//  Created:
//    13 Jan 2025, 22:03:36
//  Last edited:
//    13 Jan 2025, 22:38:08
//  Auto updated?
//    Yes
//
//  Description:
//!   Defines some middleware for implementing authentication.
//

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use base64ct::{Base64, Encoding as _};
use log::debug;

use crate::state::{AuthMode, Context};


/***** LIBRARY *****/
/// Provides HTTP Basic authentication.
///
/// # Arguments
/// - `context`: The [`Context`] that is used to check what username / password is checked for.
///
/// # Returns
/// If the user failed authentication, a 401 UNAUTHORIZED is emitted as per
/// <https://en.wikipedia.org/wiki/Basic_access_authentication#Server_side>.
///
/// Else, any [`Response`] is returned as dictacted by the `next` layer.
pub async fn basic(State(context): State<Arc<Context>>, ConnectInfo(client): ConnectInfo<SocketAddr>, request: Request, next: Next) -> Response {
    debug!(target: client.to_string().as_str(), "Evaluating basic auth");

    // Unwrap the auth mode
    let (username, password): (&str, &str) = if let AuthMode::Basic { username, password } = &context.auth {
        (username, password)
    } else {
        panic!("Calling `basic`-middleware with non-AuthMode::Basic config; this should never happen!");
    };

    // Check if we can find the auth header
    if let Some(value) = request.headers().get(&AUTHORIZATION) {
        // Check if it starts with basic
        if let Ok(value) = value.to_str() {
            if value.len() >= 6 && &value[..6] == "Basic " {
                // Attempt to parse the Base64
                let creds = &value[6..];
                if let Ok(creds) = Base64::decode_vec(creds) {
                    if let Ok(creds) = std::str::from_utf8(&creds) {
                        // Split on semicolons
                        if let Some(pos) = creds.find(':') {
                            let user: &str = &creds[..pos];
                            let pass: &str = &creds[pos + 1..];
                            if user == username && pass == password {
                                // OK! Checks out! Proceed as planned
                                debug!(target: client.to_string().as_str(), "Basic auth ACCEPTED");
                                return next.run(request).await;
                            }
                        }
                    }
                }
            }
        }
    }

    // Else, the user auth failed somehow; let them know
    debug!(target: client.to_string().as_str(), "Basic auth REJECTED");
    let mut res = Response::new(Body::empty());
    *res.status_mut() = StatusCode::UNAUTHORIZED;
    res.headers_mut().insert(WWW_AUTHENTICATE, HeaderValue::try_from("Basic realm=\"User Visible Realm\", charset=\"UTF-8\"").unwrap());
    res
}
