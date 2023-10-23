
use tokio::io;
use tokio::net::{TcpListener, TcpStream};

use async_stream::try_stream;
use futures_core::stream::Stream;


pub fn catch_sockets(addr: String, port: u16) -> impl Stream<Item = io::Result<TcpStream>> {
    try_stream! {
        let listener = TcpListener::bind(addr + ":" + &port.to_string()).await?;

        loop {
            let (socket, _) = listener.accept().await?;
            yield socket;
        }
    }
}
