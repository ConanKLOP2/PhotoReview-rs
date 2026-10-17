#![cfg(windows)]

// Compile the owned COM unit in isolation until the integration lead adds the crate-root module declaration.
#[path = "../src/explorer_com_interop.rs"]
mod explorer_com_interop;
