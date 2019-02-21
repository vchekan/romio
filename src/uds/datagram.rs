use crate::reactor::PollEvented;

use futures::task::Waker;
use futures::{ready, Poll};
use mio::Ready;
use mio_uds;

use std::fmt;
use std::io;
use std::net::Shutdown;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::SocketAddr;
use std::path::Path;

/// An I/O object representing a Unix datagram socket.
pub struct UnixDatagram {
    io: PollEvented<mio_uds::UnixDatagram>,
}

impl UnixDatagram {
    /// Creates a new `UnixDatagram` bound to the specified path.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let sock = UnixDatagram::bind("/tmp/sock")?;
    /// # Ok(()) }
    /// ```
    pub fn bind(path: impl AsRef<Path>) -> io::Result<UnixDatagram> {
        let socket = mio_uds::UnixDatagram::bind(path)?;
        Ok(UnixDatagram::new(socket))
    }

    /// Creates an unnamed pair of connected sockets.
    ///
    /// This function will create a pair of interconnected Unix sockets for
    /// communicating back and forth between one another. Each socket will be
    /// associated with the event loop whose handle is also provided.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let (sock1, sock2) = UnixDatagram::pair()?;
    /// # Ok(()) }
    /// ```
    pub fn pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
        let (a, b) = mio_uds::UnixDatagram::pair()?;
        let a = UnixDatagram::new(a);
        let b = UnixDatagram::new(b);

        Ok((a, b))
    }

    fn new(socket: mio_uds::UnixDatagram) -> UnixDatagram {
        let io = PollEvented::new(socket);
        UnixDatagram { io }
    }

    /// Creates a new `UnixDatagram` which is not bound to any address.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let sock = UnixDatagram::unbound()?;
    /// # Ok(()) }
    /// ```
    pub fn unbound() -> io::Result<UnixDatagram> {
        let socket = mio_uds::UnixDatagram::unbound()?;
        Ok(UnixDatagram::new(socket))
    }

    /// Test whether this socket is ready to be read or not.
    pub fn poll_read_ready(&self, lw: &Waker) -> Poll<io::Result<Ready>> {
        self.io.poll_read_ready(lw)
    }

    /// Test whether this socket is ready to be written to or not.
    pub fn poll_write_ready(&self, lw: &Waker) -> Poll<io::Result<Ready>> {
        self.io.poll_write_ready(lw)
    }

    /// Returns the local address that this socket is bound to.
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let stream = UnixDatagram::bind("/tmp/sock")?;
    /// let addr = stream.local_addr()?;
    /// # Ok(()) }
    /// ```
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.get_ref().local_addr()
    }

    /// Returns the address of this socket's peer.
    ///
    /// The `connect` method will connect the socket to a peer.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let stream = UnixDatagram::bind("/tmp/sock")?;
    /// let addr = stream.peer_addr()?;
    /// # Ok(()) }
    /// ```
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.io.get_ref().peer_addr()
    }

    /// Receives data from the socket.
    ///
    /// On success, returns the number of bytes read and the address from
    /// whence the data came.
    pub fn poll_recv_from(
        &self,
        lw: &Waker,
        buf: &mut [u8],
    ) -> Poll<io::Result<(usize, SocketAddr)>> {
        ready!(self.io.poll_read_ready(lw)?);

        let r = self.io.get_ref().recv_from(buf);

        if is_wouldblock(&r) {
            self.io.clear_read_ready(lw)?;
            Poll::Pending
        } else {
            Poll::Ready(r)
        }
    }

    /// Sends data on the socket to the specified address.
    ///
    /// On success, returns the number of bytes written.
    pub fn poll_send_to(
        &self,
        lw: &Waker,
        buf: &[u8],
        path: impl AsRef<Path>,
    ) -> Poll<io::Result<usize>> {
        ready!(self.io.poll_write_ready(lw)?);

        let r = self.io.get_ref().send_to(buf, path);

        if is_wouldblock(&r) {
            self.io.clear_write_ready(lw)?;
            Poll::Pending
        } else {
            Poll::Ready(r)
        }
    }

    /// Returns the value of the `SO_ERROR` option.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use romio::uds::UnixDatagram;
    ///
    /// # fn run() -> std::io::Result<()> {
    /// let stream = UnixDatagram::bind("/tmp/sock")?;
    /// if let Ok(Some(err)) = stream.take_error() {
    ///     println!("Got error: {:?}", err);
    /// }
    /// # Ok(()) }
    /// ```
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.io.get_ref().take_error()
    }

    /// Shut down the read, write, or both halves of this connection.
    ///
    /// This function will cause all pending and future I/O calls on the
    /// specified portions to immediately return with an appropriate value
    /// (see the documentation of `Shutdown`).
    ///
    /// ## Examples
    ///
    /// ```rust
    /// use romio::uds::UnixDatagram;
    /// use std::net::Shutdown;
    ///
    /// # fn run () -> Result<(), Box<dyn std::error::Error + 'static>> {
    /// let stream = UnixDatagram::bind("/tmp/sock")?;
    /// stream.shutdown(Shutdown::Both)?;
    /// # Ok(())}
    /// ```
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.io.get_ref().shutdown(how)
    }
}

impl fmt::Debug for UnixDatagram {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.io.get_ref().fmt(f)
    }
}

impl AsRawFd for UnixDatagram {
    fn as_raw_fd(&self) -> RawFd {
        self.io.get_ref().as_raw_fd()
    }
}

fn is_wouldblock<T>(r: &io::Result<T>) -> bool {
    match *r {
        Ok(_) => false,
        Err(ref e) => e.kind() == io::ErrorKind::WouldBlock,
    }
}
