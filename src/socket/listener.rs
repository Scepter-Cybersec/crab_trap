use std::io::{Error, ErrorKind};

use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tokio::sync::broadcast::Receiver as BroadcastReceiver;

use async_stream::try_stream;
use futures_core::stream::Stream;
use tokio::sync::mpsc::Receiver;
use tokio::sync::watch::Sender;
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
    mut receiver: BroadcastReceiver<&str>,
    signal: &str,
    raw_mode: &mut bool,
) -> Result<(), Error> {
    loop {
        match receiver.recv().await {
            Ok(val) => {
                if val.eq(signal) {
                    break;
                } else if val.eq("raw") {
                    *raw_mode = !*raw_mode;
                } else if val.eq("delete") {
                    return Err(Error::new(ErrorKind::Interrupted, "Delete signal received"));
                }
            }
            Err(_) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "Unknown error occurred when waiting for signal",
                ));
            }
        };
    }
    Ok(())
}

pub fn start_socket(
    mut socket: TcpStream,
    controller_sender: Sender<String>,
    mut controller_receiver: Receiver<String>,
    soc_kill_token: CancellationToken,
) {
    // let (mut read_soc, mut write_soc) = socket.into_split();
    tokio::spawn(async move {
        loop {
            let mut buf = [0; 1024];
            select! {
                recv_item = controller_receiver.recv() => {
                    match recv_item {
                        Some(val) => {
                            if socket.write(val.as_bytes()).await.is_err(){
                                println!("exit\r");
                                return;
                            }
                            socket.flush().await.unwrap();
                        }
                        None => (),
                    };
                }
                item = socket.read(&mut buf) => {
                    let n = match item {
                        // socket closed
                        Ok(n) if n == 0 => return,
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("failed to read from socket; err = {:?}", e);
                            return;
                        }
                    };
                    let send_content = String::from_utf8((&buf[0..n]).to_vec()).unwrap_or_default();
                    if controller_sender.send(send_content).is_err() {
                        return;
                    }
                }
                _ = soc_kill_token.cancelled() => {
                    return;
                }
            }
        }
    });
}
