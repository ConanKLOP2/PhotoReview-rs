//! W10-H2 only: bounded local socket server; no Internet, global proxy or trust changes.
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const GUARD: Duration = Duration::from_secs(5);
pub struct Server {
    address: SocketAddr,
    pub requests: mpsc::Receiver<String>,
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<()>,
    worker: Option<JoinHandle<()>>,
}
impl Server {
    pub fn new(mut respond: impl FnMut(usize, &mut TcpStream) + Send + 'static) -> Self {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("loopback bind");
        let address = listener.local_addr().unwrap();
        let (request_tx, requests) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            for (index, incoming) in listener.incoming().enumerate() {
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                let mut stream = incoming.expect("accept loopback");
                stream.set_read_timeout(Some(GUARD)).unwrap();
                stream.set_write_timeout(Some(GUARD)).unwrap();
                let mut bytes = Vec::new();
                let mut byte = [0];
                while !bytes.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        break;
                    }
                    bytes.push(byte[0]);
                    assert!(bytes.len() <= 64 * 1024, "bounded request headers");
                }
                if bytes.is_empty() {
                    continue;
                }
                let _ = request_tx.send(String::from_utf8(bytes).unwrap());
                respond(index, &mut stream);
                let _ = stream.shutdown(Shutdown::Both);
            }
            let _ = done_tx.send(());
        });
        Self {
            address,
            requests,
            stop,
            done,
            worker: Some(worker),
        }
    }
    pub fn url(&self) -> String {
        format!("http://{}/update", self.address)
    }
    pub fn bytes(response: Vec<u8>) -> Self {
        Self::new(move |_, stream| {
            let _ = stream.write_all(&response);
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Wake blocking accept without sleep polling; all active reads/writes/gates are bounded.
        let _ = TcpStream::connect_timeout(&self.address, GUARD);
        if self.done.recv_timeout(GUARD + GUARD).is_ok() {
            if let Some(worker) = self.worker.take() {
                worker.join().expect("loopback worker");
            }
        } else if !thread::panicking() {
            panic!("loopback cleanup exceeded hang guard");
        }
    }
}
