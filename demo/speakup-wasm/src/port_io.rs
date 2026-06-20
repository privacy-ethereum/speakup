//! An `AsyncRead`/`AsyncWrite` duplex over a `MessagePort`.
//!
//! The mux connection above wants an io object that is `Send + Sync`,
//! but `MessagePort` is neither. The port therefore never enters the io
//! object: incoming messages are pumped into an unbounded channel by the
//! port's `onmessage` callback, and outgoing writes are drained from another
//! unbounded channel by a `spawn_local` task that owns the port. [`PortIo`]
//! itself holds only the channel halves, which are `Send + Sync`.
//!
//! Messages cross the port as transferred `ArrayBuffer`s (no copies in JS).
//! Boundaries don't matter: this is a byte stream to the codec above it.
//! Writes are buffered until `poll_flush` so that one flush batch becomes one
//! `post_message` — the mux writes each frame as two `poll_write` calls
//! (header, then body) and flushes once per drive-loop pass, so per-write
//! posting would double the page-visible message count for no reason.

use std::{
    collections::VecDeque,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{AsyncRead, AsyncWrite, Stream, StreamExt, channel::mpsc};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::{JsCast, closure::Closure};
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, MessagePort};

/// A byte-stream duplex whose far end is a `MessagePort`.
pub struct PortIo {
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
    /// Bytes received but not yet read out.
    buf: VecDeque<u8>,
    /// Bytes written but not yet flushed to the port.
    wbuf: Vec<u8>,
}

/// Wires `port` into a [`PortIo`]. The port's `onmessage` is taken over and
/// the registered closure leaks — ports live for the lifetime of the worker
/// here, so nothing is lost.
pub fn port_io(port: MessagePort) -> PortIo {
    let (in_tx, incoming) = mpsc::unbounded::<Vec<u8>>();
    let (outgoing, mut out_rx) = mpsc::unbounded::<Vec<u8>>();

    let on_msg = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
        let bytes = Uint8Array::new(&ev.data()).to_vec();
        let _ = in_tx.unbounded_send(bytes);
    });
    // `set_onmessage` implicitly starts the port.
    port.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    on_msg.forget();

    spawn_local(async move {
        while let Some(bytes) = out_rx.next().await {
            let arr = Uint8Array::from(bytes.as_slice());
            let buf = arr.buffer();
            if port
                .post_message_with_transferable(&buf, &Array::of1(&buf))
                .is_err()
            {
                break;
            }
        }
    });

    PortIo {
        incoming,
        outgoing,
        buf: VecDeque::new(),
        wbuf: Vec::new(),
    }
}

impl AsyncRead for PortIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        while self.buf.is_empty() {
            match Pin::new(&mut self.incoming).poll_next(cx) {
                Poll::Ready(Some(bytes)) => self.buf.extend(bytes),
                // Sender dropped: the peer is gone — EOF.
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = out.len().min(self.buf.len());
        for (dst, b) in out.iter_mut().zip(self.buf.drain(..n)) {
            *dst = b;
        }
        Poll::Ready(Ok(n))
    }
}

impl AsyncWrite for PortIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.wbuf.extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.wbuf.is_empty() {
            return Poll::Ready(Ok(()));
        }
        let batch = std::mem::take(&mut self.wbuf);
        self.outgoing
            .unbounded_send(batch)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "port closed"))?;
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
