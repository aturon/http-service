//! Types and traits giving an interface between low-level http server implementations
//! and services that use them. The interface is based on the `std::futures` API.

#![cfg_attr(feature = "nightly", deny(missing_docs))]
#![cfg_attr(test, deny(warnings))]
#![feature(pin, futures_api, async_await, await_macro, arbitrary_self_types)]

use bytes::Bytes;
use futures::{
    future,
    prelude::*,
    stream::{self, StreamObj},
    task::LocalWaker,
    Poll,
};

use std::marker::Unpin;
use std::pin::Pin;

/// The raw body of an http request or response.
///
/// A body is a stream of `Bytes` values, which are shared handles to byte buffers.
/// Both `Body` and `Bytes` values can be easily created from standard owned byte buffer types
/// like `Vec<u8>` or `String`, using the `From` trait.
pub struct Body {
    stream: StreamObj<'static, Result<Bytes, std::io::Error>>,
}

impl Body {
    /// Create an empty body.
    pub fn empty() -> Self {
        Body::from_stream(stream::empty())
    }

    /// Create a body from a stream of `Bytes`
    pub fn from_stream<S>(s: S) -> Self
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    {
        Self {
            stream: StreamObj::new(Box::new(s)),
        }
    }
}

impl<T: Into<Bytes> + Send> From<T> for Body {
    fn from(x: T) -> Self {
        Self::from_stream(stream::once(future::ok(x.into())))
    }
}

impl Unpin for Body {}

impl Stream for Body {
    type Item = Result<Bytes, std::io::Error>;
    fn poll_next(mut self: Pin<&mut Self>, lw: &LocalWaker) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.stream).poll_next(lw)
    }
}

/// An HTTP request with a streaming body.
pub type Request = http::Request<Body>;

/// An HTTP response with a streaming body.
pub type Response = http::Request<Body>;

/// The only possible service-level error, which tells the HTTP server to
/// hang up the connection.
///
/// Any logging or other error handling should be separately arranged prior
/// to returning a `HangUp`.
#[derive(Debug, Clone)]
pub struct HangUp;

/// An async HTTP service
///
/// An instance represents a service as a whole. The associated `Conn` type
/// represents a particular connection, and may carry connection-specific state.
pub trait HttpService {
    /// An individual connection.
    ///
    /// This associated type is used to establish and hold any per-connection state
    /// needed by the service.
    type Connection;

    /// A future for setting up an individual connection.
    ///
    /// This method is called each time the server receives a new connection request,
    /// but before actually exchanging any data with the client.
    ///
    /// Returning a `HangUp` error will result in the server immediately dropping
    /// the connection.
    type ConnectionFuture: TryFuture<Ok = Self::Connection, Error = HangUp>;

    /// Initiate a new connection.
    ///
    /// This method is given access to the global service (`&self`), which may provide
    /// handles to connection pools, thread pools, or other global data.
    fn connect(&self) -> Self::ConnectionFuture;

    /// The async computation for producing the response.
    ///
    /// Returning a `HangUp` error will result in the server immediately dropping
    /// the connection. It is usually preferable to instead return an HTTP response
    /// with an error status code.
    type Fut: TryFuture<Ok = Response, Error = HangUp>;

    /// Begin handling a single request.
    ///
    /// The handler is given shared access to the service itself, and mutable access
    /// to the state for the connection where the request is taking place.
    fn respond(&self, conn: &mut Self::Connection, req: Request) -> Self::Fut;
}

impl<F, Fut> HttpService for F
where
    F: Fn(Request) -> Fut,
    Fut: TryFuture<Ok = Response, Error = HangUp>,
{
    type Connection = ();
    type ConnectionFuture = future::Ready<Result<(), HangUp>>;
    fn connect(&self) -> Self::ConnectionFuture {
        future::ok(())
    }

    type Fut = Fut;
    fn respond(&self, _: &mut (), req: Request) -> Self::Fut {
        (self)(req)
    }
}
