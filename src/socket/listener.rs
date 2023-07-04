use std::io::{Error, ErrorKind};

use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch::Receiver as WatchReceiver;

use async_stream::try_stream;
use futures_core::stream::Stream;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;

pub fn catch_sockets(addr: String, port: u16) -> impl Stream<Item = io::Result<TcpStream>> {
    try_stream! {
        let listener = TcpListener::bind(addr + ":" + &port.to_string()).await?;

        loop {
            let (socket, _) = listener.accept().await?;
            yield socket;
        }
    }
}

pub async fn wait_for_signal(
    receiver: &mut WatchReceiver<&str>,
    signal: &str,
) -> Result<(), Error> {
    while receiver.changed().await.is_ok() {
        let val = *receiver.borrow();
        if val.eq(signal) {
            break;
        } else if val.eq("delete") {
            return Err(Error::new(ErrorKind::Interrupted, "Delete signal received"));
        }
    }
    Ok(())
}

pub fn start_socket(
    socket: TcpStream,
    controller_sender: Sender<String>,
    mut controller_receiver: Receiver<String>,
    soc_kill_token: CancellationToken,
) {
    let (mut read_soc, mut write_soc) = socket.into_split();
    let read_handle = tokio::spawn(async move {
        // In a loop, read data from the socket and write the data back.
        loop {
            match controller_receiver.recv().await {
                Some(val) => {
                    write_soc.write_all(val.as_bytes()).await.unwrap();
                    write_soc.flush().await.unwrap();
                }
                None => return,
            }
        }
    });
    let write_handle = tokio::spawn(async move {
        loop {
            let mut buf = [0; 1024];
            let n = match read_soc.read(&mut buf).await {
                // socket closed
                Ok(n) if n == 0 => return,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("failed to read from socket; err = {:?}", e);
                    return;
                }
            };

            let send_content = String::from_utf8((&buf[0..n]).to_vec()).unwrap_or_default();

            if controller_sender.send(send_content).await.is_err() {
                return;
            }
        }
    });

    tokio::spawn(async move {
        soc_kill_token.cancelled().await;
        read_handle.abort();
        write_handle.abort();
    });
}
