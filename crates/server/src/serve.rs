//! Accept loop of the HTTP server: connection cap, connection timeouts and
//! graceful shutdown.
//!
//! The time limit on a whole request is not applied here: it is a layer of
//! the router (see [`REQUEST_TIMEOUT`](crate::app::REQUEST_TIMEOUT)).

use std::convert::Infallible;
use std::error::Error;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::{Pin, pin};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use hyper_util::server::graceful::GracefulShutdown;
use tokio::net::TcpListener;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tower::ServiceExt;
use tracing::{debug, error, info, warn};

/// Longest time a client may take to send the complete headers of an
/// HTTP/1 request, counted from when the server starts waiting for them.
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Longest time a connection may stay open with no request being handled.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Most connections served at the same time.
pub const MAX_CONNECTIONS: usize = 1024;

/// Longest time the shutdown waits for open connections to finish.
pub const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Pause after an accept error that is not caused by a single connection,
/// such as running out of file descriptors.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_secs(1);

/// Limits of the connections accepted by [`serve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServeConfig {
    /// HTTP/1 only: a connection that has not sent the complete headers of
    /// a request this long after the server started waiting for them is
    /// closed. The wait starts when the connection is opened and again
    /// after every response on a keep-alive connection.
    pub header_read_timeout: Duration,
    /// A connection with no request being handled for this long is closed,
    /// whatever its protocol: a client that connects and sends nothing, an
    /// idle HTTP/2 connection, an idle HTTP/1 keep-alive connection.
    pub idle_timeout: Duration,
    /// Most connections served at the same time. When it is reached the
    /// server stops accepting until a connection closes.
    pub max_connections: usize,
    /// After the shutdown signal, longest wait for the open connections to
    /// finish their requests.
    pub shutdown_drain_timeout: Duration,
}

impl Default for ServeConfig {
    /// [`HEADER_READ_TIMEOUT`], [`IDLE_TIMEOUT`], [`MAX_CONNECTIONS`] and
    /// [`SHUTDOWN_DRAIN_TIMEOUT`].
    fn default() -> Self {
        Self {
            header_read_timeout: HEADER_READ_TIMEOUT,
            idle_timeout: IDLE_TIMEOUT,
            max_connections: MAX_CONNECTIONS,
            shutdown_drain_timeout: SHUTDOWN_DRAIN_TIMEOUT,
        }
    }
}

/// Serves `app` on the connections accepted by `listener`, over HTTP/1 or
/// HTTP/2 (detected per connection), until `shutdown` resolves.
///
/// Every request carries the peer address as a
/// [`ConnectInfo<SocketAddr>`](ConnectInfo) extension.
///
/// Limits, from `config`:
/// - At most `max_connections` connections are open at once (0 counts as 1,
///   values above [`Semaphore::MAX_PERMITS`] as that maximum). At the cap
///   the server does not accept: further clients wait in the listen
///   backlog.
/// - An HTTP/1 connection that does not send complete request headers
///   within `header_read_timeout` is closed.
/// - A connection with no request in handling for `idle_timeout` is closed.
///   A request stops counting when its response headers are ready, so a
///   response body still being written when the timeout expires is cut.
///
/// An accept error does not stop the server: it is logged, and when it is
/// not caused by a single connection (for example no file descriptors
/// left) the loop pauses for a second before accepting again.
///
/// When `shutdown` resolves the listener is closed, idle connections are
/// closed and open ones finish their requests. After
/// `shutdown_drain_timeout` the function returns even if some connection is
/// still open: those connection tasks keep running until the Tokio runtime
/// is dropped.
pub async fn serve<F>(listener: TcpListener, app: Router, config: ServeConfig, shutdown: F)
where
    F: Future<Output = ()> + Send,
{
    let max_connections = config.max_connections.clamp(1, Semaphore::MAX_PERMITS);
    let connections = Arc::new(Semaphore::new(max_connections));
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(config.header_read_timeout);
    builder.http2().enable_connect_protocol();
    let graceful = GracefulShutdown::new();
    let mut shutdown = pin!(shutdown);

    loop {
        let permit = tokio::select! {
            permit = Arc::clone(&connections).acquire_owned() => permit,
            () = &mut shutdown => break,
        };
        // The semaphore is never closed.
        let Ok(permit) = permit else {
            break;
        };
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(err) => {
                    handle_accept_error(&err).await;
                    continue;
                }
            },
            () = &mut shutdown => break,
        };
        let in_flight = Arc::new(watch::Sender::new(0_usize));
        let service = ConnectionService {
            app: app.clone(),
            peer,
            in_flight: Arc::clone(&in_flight),
        };
        let connection = builder
            .serve_connection_with_upgrades(TokioIo::new(stream), service)
            .into_owned();
        let connection = graceful.watch(connection);
        tokio::spawn(drive_connection(
            connection,
            in_flight,
            config.idle_timeout,
            permit,
        ));
    }

    drop(listener);
    info!("shutting down, waiting for open connections");
    if tokio::time::timeout(config.shutdown_drain_timeout, graceful.shutdown())
        .await
        .is_err()
    {
        warn!(
            timeout = ?config.shutdown_drain_timeout,
            "connections still open after the shutdown timeout, stopping anyway"
        );
    }
}

/// Logs an accept error. An error caused by a single connection (refused,
/// aborted or reset by the peer) is logged at debug level; any other error
/// is logged as an error and followed by a pause of
/// [`ACCEPT_ERROR_BACKOFF`], so that a persistent error such as running
/// out of file descriptors does not spin the accept loop.
async fn handle_accept_error(err: &io::Error) {
    let single_connection = matches!(
        err.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    );
    if single_connection {
        debug!(%err, "accept error");
        return;
    }
    error!(%err, "accept error, retrying shortly");
    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
}

/// Drives `connection` to the end, or drops it once no request has been in
/// handling for `idle_timeout`. `in_flight` holds the number of requests in
/// handling on the connection. `_permit` is the connection slot, released
/// when this returns.
async fn drive_connection<C>(
    connection: C,
    in_flight: Arc<watch::Sender<usize>>,
    idle_timeout: Duration,
    _permit: OwnedSemaphorePermit,
) where
    C: Future<Output = Result<(), Box<dyn Error + Send + Sync>>>,
{
    let mut connection = pin!(connection);
    let mut requests = in_flight.subscribe();
    loop {
        let idle = *requests.borrow_and_update() == 0;
        tokio::select! {
            result = connection.as_mut() => {
                if let Err(err) = result {
                    debug!(%err, "connection closed with an error");
                }
                return;
            }
            changed = requests.changed() => {
                // `in_flight` keeps a sender alive, so this never fails.
                if changed.is_err() {
                    return;
                }
            }
            () = tokio::time::sleep(idle_timeout), if idle => {
                debug!("closing idle connection");
                return;
            }
        }
    }
}

/// Per-connection service: adds the peer address to each request, counts
/// the requests in handling and hands them to the router.
#[derive(Clone)]
struct ConnectionService {
    app: Router,
    peer: SocketAddr,
    in_flight: Arc<watch::Sender<usize>>,
}

impl hyper::service::Service<Request<Incoming>> for ConnectionService {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn call(&self, mut request: Request<Incoming>) -> Self::Future {
        request.extensions_mut().insert(ConnectInfo(self.peer));
        let in_handling = InFlightRequest::start(Arc::clone(&self.in_flight));
        let response = self.app.clone().oneshot(request);
        Box::pin(async move {
            let response = response.await;
            drop(in_handling);
            response
        })
    }
}

/// One request counted in the in-flight count of its connection until it
/// is dropped.
struct InFlightRequest(Arc<watch::Sender<usize>>);

impl InFlightRequest {
    fn start(count: Arc<watch::Sender<usize>>) -> Self {
        count.send_modify(|n| *n = n.saturating_add(1));
        Self(count)
    }
}

impl Drop for InFlightRequest {
    fn drop(&mut self) {
        self.0.send_modify(|n| *n = n.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use axum::http::StatusCode;
    use axum::routing::get;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;
    use tower_http::timeout::TimeoutLayer;

    use super::*;

    const SHORT: Duration = Duration::from_millis(300);
    const LONG: Duration = Duration::from_secs(60);
    /// Upper bound on any wait for the server, far above every timeout the
    /// tests configure.
    const PATIENCE: Duration = Duration::from_secs(5);
    const GET_OK: &[u8] = b"GET /ok HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";

    fn config() -> ServeConfig {
        ServeConfig {
            header_read_timeout: LONG,
            idle_timeout: LONG,
            max_connections: MAX_CONNECTIONS,
            shutdown_drain_timeout: LONG,
        }
    }

    struct TestServer {
        addr: SocketAddr,
        stop: oneshot::Sender<()>,
        task: JoinHandle<()>,
    }

    async fn start(app: Router, config: ServeConfig) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(serve(listener, app, config, async {
            let _ = stopped.await;
        }));
        TestServer { addr, stop, task }
    }

    fn ok_app() -> Router {
        Router::new().route("/ok", get(|| async { "ok" }))
    }

    fn slow_app(delay: Duration) -> Router {
        Router::new().route(
            "/slow",
            get(move || async move {
                tokio::time::sleep(delay).await;
                "done"
            }),
        )
    }

    /// Reads from `stream` until the server closes it, failing after
    /// [`PATIENCE`].
    async fn read_until_closed(stream: &mut TcpStream) -> String {
        let mut response = Vec::new();
        tokio::time::timeout(PATIENCE, stream.read_to_end(&mut response))
            .await
            .expect("the server did not close the connection")
            .unwrap();
        String::from_utf8_lossy(&response).into_owned()
    }

    async fn send(addr: SocketAddr, request: &[u8]) -> String {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(request).await.unwrap();
        read_until_closed(&mut stream).await
    }

    #[tokio::test]
    async fn closes_connection_with_partial_headers_after_header_timeout() {
        let server = start(
            ok_app(),
            ServeConfig {
                header_read_timeout: SHORT,
                ..config()
            },
        )
        .await;
        let mut stream = TcpStream::connect(server.addr).await.unwrap();
        let sent_at = Instant::now();

        stream
            .write_all(b"GET /ok HTTP/1.1\r\nHost: x\r\nX-A: ")
            .await
            .unwrap();
        let response = read_until_closed(&mut stream).await;

        assert!(sent_at.elapsed() >= SHORT - Duration::from_millis(50));
        assert!(!response.contains("200 OK"));
    }

    #[tokio::test]
    async fn closes_silent_connection_after_idle_timeout() {
        let server = start(
            ok_app(),
            ServeConfig {
                idle_timeout: SHORT,
                ..config()
            },
        )
        .await;
        let mut stream = TcpStream::connect(server.addr).await.unwrap();
        let connected_at = Instant::now();

        let response = read_until_closed(&mut stream).await;

        assert!(connected_at.elapsed() >= SHORT - Duration::from_millis(50));
        assert_eq!(response, "");
    }

    #[tokio::test]
    async fn answers_408_when_the_router_timeout_expires() {
        let app = slow_app(LONG).layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            SHORT,
        ));
        let server = start(app, config()).await;

        let response = send(
            server.addr,
            b"GET /slow HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
        )
        .await;

        assert!(response.starts_with("HTTP/1.1 408"), "{response}");
    }

    #[tokio::test]
    async fn passes_the_peer_address_as_connect_info() {
        let app = Router::new().route(
            "/peer",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.to_string() }),
        );
        let server = start(app, config()).await;
        let mut stream = TcpStream::connect(server.addr).await.unwrap();
        let local = stream.local_addr().unwrap();

        stream
            .write_all(b"GET /peer HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with(&local.to_string()), "{response}");
    }

    // With a cap of one, the second client is not served while the first
    // connection is open, and is served as soon as it closes.
    #[tokio::test]
    async fn serves_next_connection_only_after_one_closes_at_the_cap() {
        let server = start(
            ok_app(),
            ServeConfig {
                max_connections: 1,
                ..config()
            },
        )
        .await;
        let first = TcpStream::connect(server.addr).await.unwrap();
        // Let the server accept the first connection before the second one.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut second = TcpStream::connect(server.addr).await.unwrap();
        second.write_all(GET_OK).await.unwrap();

        let mut byte = [0_u8; 1];
        let early = tokio::time::timeout(SHORT, second.read(&mut byte)).await;
        drop(first);
        let response = read_until_closed(&mut second).await;

        assert!(early.is_err(), "second connection served at the cap");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    }

    #[tokio::test]
    async fn finishes_in_flight_request_on_shutdown() {
        let server = start(slow_app(SHORT), config()).await;
        let mut stream = TcpStream::connect(server.addr).await.unwrap();
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        // Let the request reach the handler before the shutdown starts.
        tokio::time::sleep(Duration::from_millis(100)).await;

        server.stop.send(()).unwrap();
        let response = read_until_closed(&mut stream).await;

        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("done"), "{response}");
    }

    #[tokio::test]
    async fn stops_after_drain_timeout_with_a_stuck_request() {
        let server = start(
            slow_app(LONG),
            ServeConfig {
                shutdown_drain_timeout: SHORT,
                ..config()
            },
        )
        .await;
        let mut stream = TcpStream::connect(server.addr).await.unwrap();
        stream
            .write_all(b"GET /slow HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        server.stop.send(()).unwrap();
        let stopped = tokio::time::timeout(PATIENCE, server.task).await;

        assert!(
            stopped.is_ok(),
            "serve did not return after the drain timeout"
        );
    }

    #[tokio::test]
    async fn stops_accepting_after_shutdown() {
        let server = start(ok_app(), config()).await;

        server.stop.send(()).unwrap();
        tokio::time::timeout(PATIENCE, server.task)
            .await
            .unwrap()
            .unwrap();

        assert!(TcpStream::connect(server.addr).await.is_err());
    }
}
