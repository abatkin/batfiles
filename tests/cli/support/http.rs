//! Loopback HTTP servers and transfer-failure fixtures.

use std::io::{Cursor, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// What the local server answers one path with.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    /// The file itself.
    Body(&'static str),
    /// An owned binary response body.
    Bytes(Vec<u8>),
    /// A path the server does not have, answered 404.
    Missing,
    /// A response that is not a complete file, such as HTTP 204 or 206.
    NotAWholeFile { status: u16, body: &'static str },
    /// A permanent redirect to another path on the same server.
    RedirectTo(&'static str),
    /// A redirect with this status to this `Location`, as GitHub answers `releases/latest`.
    Redirect { status: u16, location: String },
}

/// A loopback HTTP server with configurable responses and request counting.
pub(crate) struct Server {
    server: Arc<tiny_http::Server>,
    address: String,
    routes: Arc<Mutex<Vec<(String, Reply)>>>,
    requests: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    /// Start a server answering each named path with the reply beside it.
    pub(crate) fn new(routes: &[(&'static str, Reply)]) -> Self {
        Self::serving(
            routes
                .iter()
                .map(|(path, reply)| ((*path).to_owned(), reply.clone()))
                .collect(),
        )
    }

    /// Start a server answering each path, built at run time, with the reply beside it.
    pub(crate) fn serving(routes: Vec<(String, Reply)>) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("a local HTTP server"));
        let address = format!("http://{}", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));
        let routes = Arc::new(Mutex::new(routes));

        let worker = {
            let server = Arc::clone(&server);
            let requests = Arc::clone(&requests);
            let routes = Arc::clone(&routes);
            std::thread::spawn(move || {
                // `Drop` unblocks the server so the request thread can exit.
                for request in server.incoming_requests() {
                    requests.fetch_add(1, Ordering::SeqCst);
                    // The first matching route wins, allowing callers to override routes by
                    // prepending them.
                    let reply = routes
                        .lock()
                        .expect("the routes")
                        .iter()
                        .find(|(path, _)| *path == request.url())
                        .map_or(Reply::Missing, |(_, reply)| reply.clone());
                    let _ = request.respond(response(reply));
                }
            })
        };

        Self {
            server,
            address,
            routes,
            requests,
            worker: Some(worker),
        }
    }

    /// Answer `path` with `reply` from now on, ahead of any route given before.
    pub(crate) fn answer(&self, path: &str, reply: Reply) {
        self.routes
            .lock()
            .expect("the routes")
            .insert(0, (path.to_owned(), reply));
    }

    /// Where the server is, as a manifest writes it: `http://127.0.0.1:<port>`.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// How many requests have reached it.
    pub(crate) fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One reply, as tiny_http sends it.
fn response(reply: Reply) -> tiny_http::Response<Cursor<Vec<u8>>> {
    match reply {
        Reply::Body(body) => tiny_http::Response::from_string(body),
        Reply::Bytes(body) => tiny_http::Response::from_data(body),
        Reply::Missing => tiny_http::Response::from_string("no such file\n").with_status_code(404),
        Reply::NotAWholeFile { status, body } => {
            tiny_http::Response::from_string(body).with_status_code(status)
        }
        Reply::RedirectTo(path) => tiny_http::Response::from_string("moved\n")
            .with_status_code(301)
            .with_header(
                tiny_http::Header::from_bytes("Location", path).expect("a location header"),
            ),
        Reply::Redirect { status, location } => tiny_http::Response::from_string("found\n")
            .with_status_code(status)
            .with_header(
                tiny_http::Header::from_bytes("Location", location).expect("a location header"),
            ),
    }
}

/// An address nothing listens on, so a connection to it is refused.
pub(crate) fn refusing_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local socket");
    let address = format!("http://{}", listener.local_addr().expect("its address"));
    drop(listener);
    address
}

/// Accept every connection and never answer. Return the server URL.
pub(crate) fn server_that_never_answers() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local socket");
    let address = format!("http://{}", listener.local_addr().expect("its address"));
    std::thread::spawn(move || {
        // Hold every connection open until the test process exits.
        let mut held = Vec::new();
        for connection in listener.incoming() {
            held.push(connection);
        }
    });
    address
}

/// Serve `body` with an overstated content length, then close the connection. Return the server
/// URL.
pub(crate) fn server_that_hangs_up(body: &'static str, promised: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local socket");
    let address = format!("http://{}", listener.local_addr().expect("its address"));
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("a connection");
        let _ = socket.read(&mut [0u8; 1024]);
        let _ = write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {promised}\r\nConnection: close\r\n\r\n{body}"
        );
        let _ = socket.flush();
    });
    address
}
