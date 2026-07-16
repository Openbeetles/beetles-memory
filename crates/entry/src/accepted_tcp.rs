use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// A TCP stream inseparably bound to peer identity returned by `TcpListener::accept`.
///
/// There is intentionally no constructor from `TcpStream` or `SocketAddr`: production
/// transports must obtain this value from the listener that accepted the connection.
#[derive(Debug)]
pub struct EntryAcceptedTcpStream {
    stream: TcpStream,
    peer_addr: SocketAddr,
}

impl EntryAcceptedTcpStream {
    pub fn accept(listener: &TcpListener) -> io::Result<Self> {
        let (stream, accepted_peer_addr) = listener.accept()?;
        let observed_peer_addr = stream.peer_addr()?;
        if observed_peer_addr != accepted_peer_addr {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "accepted TCP peer identity changed before authority binding",
            ));
        }
        Ok(Self {
            stream,
            peer_addr: accepted_peer_addr,
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.stream.local_addr()
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.stream.set_write_timeout(timeout)
    }

    pub fn try_clone_transport(&self) -> io::Result<TcpStream> {
        self.stream.try_clone()
    }

    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.stream.shutdown(how)
    }

    /// Discards only bytes already queued by the kernel without waiting for a
    /// peer. HTTP ingress uses this after writing an early auth rejection so a
    /// close does not turn the response into a reset because of unread input.
    pub fn discard_currently_buffered_input(&mut self) -> io::Result<()> {
        self.stream.set_nonblocking(true)?;
        let result = (|| {
            let mut buffer = [0_u8; 8192];
            loop {
                match self.stream.read(&mut buffer) {
                    Ok(0) => return Ok(()),
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) => return Err(error),
                }
            }
        })();
        let restore = self.stream.set_nonblocking(false);
        result.and(restore)
    }

    pub(crate) const fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

impl Read for EntryAcceptedTcpStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buffer)
    }
}

impl Write for EntryAcceptedTcpStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
