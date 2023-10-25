use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rustyline::history::MemHistory;
use rustyline::{Config, Editor};
use sha256::digest;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::input::input::CompletionHelper;

#[derive(Clone)]
pub struct Handle {
    pub readline: Arc<Mutex<Editor<CompletionHelper, MemHistory>>>,
    pub read_stream: Arc<Mutex<OwnedReadHalf>>,
    pub write_stream: Arc<Mutex<OwnedWriteHalf>>,
    pub raw_mode: bool,
}

impl Handle {
    pub fn new(read_stream: OwnedReadHalf, write_stream: OwnedWriteHalf) -> Handle {
        let history = MemHistory::new();
        let mut builder = Config::builder();
        builder = builder.check_cursor_position(false);
        let config = builder.build();
        let mut rl = Editor::with_history(config, history).unwrap();
        rl.set_helper(Some(CompletionHelper::new_only_hinter()));
        let handle = Handle {
            readline: Arc::new(Mutex::new(rl)),
            read_stream: Arc::new(Mutex::new(read_stream)),
            write_stream: Arc::new(Mutex::new(write_stream)),
            raw_mode: false,
        };
        return handle;
    }
}

pub async fn soc_is_shell(
    read_stream: Arc<Mutex<OwnedReadHalf>>,
    write_stream: Arc<Mutex<OwnedWriteHalf>>,
    soc_key: String,
) -> bool {
    let read_soc = read_stream.lock().await;
    let mut write_soc = write_stream.lock().await;
    write_soc
        .write(format!("echo {}\r\n", soc_key).as_bytes())
        .await
        .unwrap();
    let mut buf: [u8; 4096] = [0; 4096];
    for _ in 0..10 {
        let len = read_soc.try_read(&mut buf);
        if len.is_ok() {
            let content: String = String::from_utf8_lossy(&buf[..len.unwrap()]).into();
            if content.contains(&soc_key) {
                // add a new line so we get our prompt back, also check to make sure the socket did not just close
                write_soc.write("\n".as_bytes()).await.unwrap_or_default();
                return true;
            }
        }
        sleep(Duration::from_millis(200)).await;
    }
    return false;
}

pub async fn handle_new_shell(
    soc: TcpStream,
    connected_shells: Arc<Mutex<HashMap<String, Handle>>>,
    skip_validation: Option<bool>,
) -> bool {
    let soc_addr = soc.peer_addr();
    let (soc_read, soc_write) = soc.into_split();
    let handle = Handle::new(soc_read, soc_write);

    let soc_key: String;
    match soc_addr {
        Ok(val) => {
            soc_key = digest(val.to_string())[0..31].to_string();
            let is_shell = match skip_validation {
                Some(true) => true,
                _ => {
                    soc_is_shell(
                        handle.read_stream.clone(),
                        handle.write_stream.clone(),
                        soc_key.clone(),
                    )
                    .await
                }
            };
            if is_shell {
                let mut shells = connected_shells.lock().await;
                shells.insert(soc_key, handle);
            } else {
                return false;
            }
        }
        Err(_) => return false,
    }
    return true;
}

#[cfg(test)]
mod test {
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn test_soc_is_shell() {
        let listener_res = TcpListener::bind("127.0.0.1:32423").await;
        assert!(listener_res.is_ok());
        let listener = listener_res.unwrap();
        tokio::spawn(async move {
            let (mut soc, _) = listener.accept().await.unwrap();
            soc.write("test123\n".as_bytes()).await.unwrap();
        });
        let (read, write) = TcpStream::connect("127.0.0.1:32423")
            .await
            .unwrap()
            .into_split();
        let read_half = Arc::new(Mutex::new(read));
        let write_half = Arc::new(Mutex::new(write));
        assert!(soc_is_shell(read_half, write_half, String::from("test123")).await);
    }

    #[tokio::test]
    async fn test_handle_new_shell() {
        let listener_res = TcpListener::bind("127.0.0.1:32424").await;
        assert!(listener_res.is_ok());
        let listener = listener_res.unwrap();
        let handle_init = tokio::spawn(async move {
            let (soc, _) = listener.accept().await.unwrap();
            let connected_shells = Arc::new(Mutex::new(HashMap::<String, Handle>::new()));
            handle_new_shell(soc, connected_shells.clone(), Some(true)).await;
            assert!(connected_shells.lock().await.len() > 0);
        });
        TcpStream::connect("127.0.0.1:32424").await.unwrap();
        handle_init.await.unwrap();
    }
}
