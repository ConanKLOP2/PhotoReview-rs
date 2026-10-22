#![allow(unused_variables, dead_code)] // Contract stubs; porter removes with the last todo.
//! W10-http: WinHTTP adapter for the C# UpdateChecker request.

pub mod policy;
pub mod transport;

pub use transport::WinHttpTransport;
