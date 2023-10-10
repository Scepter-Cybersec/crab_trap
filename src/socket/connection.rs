use std::io::{stdin, stdout};

use crate::listener;
use termion::clear;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::task;
use tokio::select;
use tokio::sync::broadcast::{self, Receiver as HandleReceiver, Sender as HandleSender};
use tokio::sync::mpsc::{self, Sender};
use tokio::sync::watch::Receiver;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Handle {
    pub tx: HandleSender<&'static str>,
    pub soc_kill_token: CancellationToken,
    pub raw_mode: bool,
}

async fn handle_key_input() -> Option<Key> {
    let (tx, mut rx) = mpsc::channel(1024);
    // stdin().keys() blocks the main thread so we have to spawn a new one and run it there
    task::spawn(async move {
        let key_input = stdin().keys().next();
        tx.send(key_input).await.unwrap();
    });
    let key_res = rx.recv().await.unwrap();
    return match key_res {
        Some(key) => {
            return match key {
                Ok(val) => Some(val),
                Err(_) => None,
            };
        }
        None => None,
    };
}

impl Handle {
    pub fn new() -> (Handle, HandleReceiver<&'static str>, CancellationToken) {
        let (tx, rx) = broadcast::channel::<&str>(1024);
        let soc_kill_token = CancellationToken::new();
        let soc_kill_token_listen = soc_kill_token.clone();
        let handle = Handle {
            tx,
            soc_kill_token,
            raw_mode: false,
        };
        return (handle, rx, soc_kill_token_listen);
    }

    pub fn handle_listen(
        &self,
        handle_to_soc_send: Sender<String>,
        mut soc_to_handle_recv: Receiver<String>,
        menu_channel_release: Sender<()>,
    ) {
        let menu_channel_release_1 = menu_channel_release.clone();
        let tx = self.tx.clone();
        let tx_copy = self.tx.clone();
        let mut raw_mode = self.raw_mode;
        // start reader
        tokio::spawn(async move {
            let mut active = false;
            loop {
                let rx1 = tx_copy.subscribe();
                let rx2 = tx_copy.subscribe();
                if !active {
                    if listener::wait_for_signal(rx1, "start", &mut raw_mode)
                        .await
                        .is_err()
                    {
                        return;
                    }
                    active = true;
                }

                // wait for new read content or pause notification
                select! {
                    _ = soc_to_handle_recv.changed() =>{
                        let resp = soc_to_handle_recv.borrow().to_owned();
                        let mut stdout = io::stdout();
                        stdout.write_all(resp.as_bytes()).await.unwrap();
                        stdout.flush().await.unwrap();
                    }
                    _ = listener::wait_for_signal(rx2, "quit", &mut raw_mode) =>{
                        active = false
                    }
                }
            }
        });
        // start writer
        tokio::spawn(async move {
            // wait for start signal
            if listener::wait_for_signal(tx.subscribe(), "start", &mut raw_mode)
                .await
                .is_err()
            {
                return;
            }

            let mut reader = BufReader::new(tokio::io::stdin());
            loop {
                let stdout = stdout().into_raw_mode().unwrap();
                if !raw_mode {
                    stdout.suspend_raw_mode().unwrap();
                    let mut buffer = Vec::new();
                    reader
                        .read_until(b'\n', &mut buffer)
                        .await
                        .unwrap_or_default();
                    let mut content = match String::from_utf8(buffer) {
                        Ok(val) => val,
                        Err(_) => continue,
                    };
                    if content.trim_end().eq("back") {
                        println!("{clear}", clear = clear::BeforeCursor);
                        //notify the reader that we're pausing
                        tx.send("quit").unwrap();
                        menu_channel_release_1.send(()).await.unwrap();
                        // send a new line so we get a prompt when we return
                        content = String::from("\n");
                        if listener::wait_for_signal(tx.subscribe(), "start", &mut raw_mode)
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    if handle_to_soc_send.send(content).await.is_err() {
                        return;
                    }
                } else {
                    let inp_val = handle_key_input().await;
                    if inp_val.is_none() {
                        continue;
                    }
                    let key = inp_val.unwrap();
                    match key {
                        Key::Ctrl('b') => {
                            println!("{clear}", clear = clear::BeforeCursor);
                            tx.send("quit").unwrap();
                            menu_channel_release_1.send(()).await.unwrap();
                            stdout.suspend_raw_mode().unwrap();
                            if listener::wait_for_signal(tx.subscribe(), "start", &mut raw_mode)
                                .await
                                .is_err()
                            {
                                return;
                            }
                            if !raw_mode {
                                continue;
                            }
                            stdout.activate_raw_mode().unwrap();
                            handle_to_soc_send.send(String::from("\n")).await.unwrap()
                        }
                        Key::Ctrl('c') => {
                            handle_to_soc_send.send(String::from("^c")).await.unwrap();
                        }
                        Key::Char(c) => {
                            handle_to_soc_send.send(String::from(c)).await.unwrap();
                        }
                        _ => (),
                    }
                }
            }
        });
    }
}
