//! Multiplayer (platform/docs/multiplayer.md): the wire protocol, the chat, the per-seat view and the
//! authoritative match server. Transport-agnostic and free of threads, sockets and clocks: the
//! native server (`net-server`), a native client, the browser and Android all drive these types
//! with bytes and a number of milliseconds.

pub mod audit;
pub mod chat;
pub mod client;
pub mod proto;
pub mod server;
pub mod view;
pub mod wire;
