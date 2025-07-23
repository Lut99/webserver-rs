//  LIB.rs
//    by Lut99
//
//  Created:
//    17 Jul 2024, 18:59:08
//  Last edited:
//    13 Jan 2025, 22:03:54
//  Auto updated?
//    Yes
//
//  Description:
//!   An `axum-`based webserver that can be used to do simple static website
//!   hosting.
//

// Declare modules
pub mod auth;
pub mod context;
pub mod server;
pub mod www;

// Use some of it in the main namespace
pub use server::{Error, Server};
