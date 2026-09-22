//! Loopback HTTP servers and transfer-failure fixtures.

use std::io::{Cursor, Read as _, Write as _};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

/// What the local server answers one path with.
#[derive(Debug, Clone)]
pub(crate) enum Reply {
    /// The file itself.
    Body(&'static str),
    /// A body that is not text, which is what an archive is. Owned rather than
    /// borrowed because the archives these tests serve are built at run time:
    /// several of them could not be committed to the repository at all, holding
    /// entry paths that escape the tree they are unpacked into.
    Bytes(Vec<u8>),
    /// A path the server does not have, answered 404.
    Missing,
    /// An answer that is not a refusal and not a whole file either: a 204 with
    /// nothing in it, or a 206 holding one range of one.
    NotAWholeFile { status: u16, body: &'static str },
    /// A permanent move to another path on the same server, which is what a
    /// release URL does before it hands over a file.
    RedirectTo(&'static str),
}

/// A local HTTP server, so no test reaches the network (`architecture.md`, "Test
/// environments").
pub(crate) struct Server {
    server: Arc<tiny_http::Server>,
    address: String,
    requests: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    /// Start a server answering each named path with the reply beside it.
    pub(crate) fn new(routes: &[(&'static str, Reply)]) -> Self {
        let routes = routes.to_vec();
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("a local HTTP server"));
        let address = format!("http://{}", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));

        let worker = {
            let server = Arc::clone(&server);
            let requests = Arc::clone(&requests);
            std::thread::spawn(move || {
                // Ends when `unblock` is called from `drop`, which is what
                // stops the thread outliving the test that started it.
                for request in server.incoming_requests() {
                    requests.fetch_add(1, Ordering::SeqCst);
                    // The first match wins, so a caller that prepends a route
                    // answers that path differently without rebuilding the set.
                    let reply = routes
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
            requests,
            worker: Some(worker),
        }
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
    }
}

/// A server that promises a length, sends less than it, and hangs up, for the
/// one test about a transfer that does not finish.
pub(crate) fn server_that_hangs_up(body: &'static str, promised: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local socket");
    let address = format!("http://{}", listener.local_addr().expect("its address"));
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("a connection");
        // Enough of the request to have read it; what it asks for does not
        // change the answer.
        let _ = socket.read(&mut [0u8; 1024]);
        let _ = write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {promised}\r\nConnection: close\r\n\r\n{body}"
        );
        let _ = socket.flush();
    });
    address
}
