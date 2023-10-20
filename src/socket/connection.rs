use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;

pub struct Handle<B>
where
    B: AsyncRead + AsyncWrite + Unpin,
{
    pub stream: Arc<Mutex<B>>,
    pub raw_mode: bool,
}

impl<B: AsyncRead + AsyncWrite + Unpin> Handle<B> {
    pub fn new(stream: B) -> Handle<B> {
        let handle = Handle {
            stream: Arc::new(Mutex::new(stream)),
            raw_mode: false,
        };
        return handle;
    }
}

impl<B: AsyncRead + AsyncWrite + Unpin> Clone for Handle<B> {
    fn clone(&self) -> Self {
        Handle {
            stream: self.stream.clone(),
            raw_mode: self.raw_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    // use termion::raw::IntoRawMode;
    // use tokio::{
    //     io::AsyncWriteExt,
    //     net::{TcpListener, TcpStream},
    // };

    // use super::*;

    #[tokio::test]
    async fn test_handle() {
        // let listener_res = TcpListener::bind("127.0.0.1:32426").await;
        // assert!(listener_res.is_ok());
        // let listener = listener_res.unwrap();
        // tokio::spawn(async move {
        //     let (mut tcp_stream, _) = listener.accept().await.unwrap();
        //     //mock return vale from soc
        //     tcp_stream.write("mock value".as_bytes()).await.unwrap();
        // });
        // let stream = TcpStream::connect("127.0.0.1:32426").await.unwrap();
        // let (handle, cancel_token) = Handle::new();
        // let (handle_to_soc_send, handle_to_soc_recv) = mpsc::channel::<String>(1024);
        // let (soc_to_handle_send, soc_to_handle_recv) = watch::channel::<String>(String::from(""));
        // let out = std::io::Cursor::new(Vec::new()).into_raw_mode().unwrap();
        // listener::start_socket(stream, soc_to_handle_send, handle_to_soc_recv, cancel_token);
        // handle.handle_listen(handle_to_soc_send.clone(), soc_to_handle_recv.clone(), out);
        // let mut rx = handle.tx.subscribe();

        // //test handle channel send/receive
        // tokio::spawn(async move {
        //     assert_eq!(rx.recv().await.unwrap(), "start");
        // });
        // handle.tx.send("start").unwrap();

        // soc_to_handle_recv.clone().changed().await.unwrap();
        // assert_eq!("mock value", soc_to_handle_recv.borrow().as_str());
    }
}
