//! A stream multiplexer over a `MessagePort`.
//!
//! `Context::new_single_threaded` runs every sub-task sequentially over one
//! shared channel. Wrapping the port in a [`tlsn_mux::Connection`] instead
//! gives each sub-task (each KOS/Ferret sub-protocol, each context fork) its
//! own logical stream, so they interleave their I/O cooperatively — the same
//! transport shape a production deployment uses.
//!
//! The [`Connection`] itself must be driven; [`port_mux`] spawns a
//! `spawn_local` task that polls it for the lifetime of the worker. The
//! [`Handle`] it leaves behind is `Send + Sync` and opens streams
//! synchronously, which is exactly what mpz's [`Mux`] trait wants.

use futures::future::poll_fn;
use mpz_common::{io::Io, mux::Mux};
use tlsn_mux::{Config, Connection, Handle};
use wasm_bindgen_futures::spawn_local;
use web_sys::MessagePort;

use crate::port_io::port_io;

/// A [`Mux`] whose far end is a `MessagePort`.
#[derive(Clone)]
pub struct PortMux {
    handle: Handle,
}

impl Mux for PortMux {
    fn open(&self, id: &[u8]) -> Result<Io, std::io::Error> {
        let stream = self.handle.new_stream(id).map_err(std::io::Error::other)?;
        Ok(Io::from_io(stream))
    }
}

/// Wires `port` into a [`PortMux`], spawning the connection driver.
pub fn port_mux(port: MessagePort) -> Result<PortMux, std::io::Error> {
    let mut conn = Connection::new(port_io(port), Config::default());
    let handle = conn.handle().map_err(std::io::Error::other)?;
    spawn_local(async move {
        let _ = poll_fn(|cx| conn.poll(cx)).await;
    });
    Ok(PortMux { handle })
}
