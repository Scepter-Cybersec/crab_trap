use std::io::{stdin, stdout};
use std::sync::Arc;

use crate::listener;
use rustyline::history::FileHistory;
use rustyline::{Config, Editor};
use termion::clear;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use tokio::io::{self, AsyncWriteExt};
use tokio::select;
use tokio::sync::broadcast::{self, Receiver as HandleReceiver, Sender as HandleSender};
use tokio::sync::mpsc::{self, Sender};
use tokio::sync::watch::{self, Receiver};
use tokio::sync::Mutex;
use tokio::task;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Handle {
    pub rl: Arc<Mutex<Editor<(), FileHistory>>>,
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

async fn read_line(rl: Arc<Mutex<Editor<(), FileHistory>>>, prompt: Option<&str>) -> String {
    let (tx, mut rx) = mpsc::channel::<String>(1024);
    let input_prompt = match prompt {
        Some(val) => String::from(val),
        None => String::from(""),
    };
    task::spawn(async move {
        let mut reader = rl.lock().await;

        let raw_content = reader.readline(&input_prompt);

        let content = match raw_content {
            Ok(line) => {
                reader.add_history_entry(line.clone()).unwrap_or_default();
                line + "\n"
            }
            Err(_) => String::from(""),
        };
        tx.send(content).await.unwrap_or_default();
    });
    let received_content = rx.recv().await.unwrap_or_default();
    return received_content;
}

impl Handle {
    pub fn new() -> (Handle, HandleReceiver<&'static str>, CancellationToken) {
        let (tx, rx) = broadcast::channel::<&str>(1024);
        let soc_kill_token = CancellationToken::new();
        let soc_kill_token_listen = soc_kill_token.clone();
        let mut builder = Config::builder();
        builder = builder.check_cursor_position(true);
        let config = builder.build();
        let rl = Arc::new(Mutex::new(
            Editor::<(), FileHistory>::with_config(config).unwrap(),
        ));
        let handle = Handle {
            rl,
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
        let rl = self.rl.clone();
        let tx_copy = self.tx.clone();
        let mut raw_mode = self.raw_mode;
        let (prompt_tx, mut prompt_rx) = watch::channel(String::from(""));
        // start reader
        tokio::spawn(async move {
            let mut active = false;

            loop {
                if !active {
                    if listener::wait_for_signal(tx_copy.subscribe(), "start", &mut raw_mode)
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
                        let outp =match raw_mode{
                            true =>resp,
                            false => format!("{clear}\r{resp}", clear = clear::CurrentLine)
                        };
                        stdout.write_all(outp.as_bytes()).await.unwrap();
                        stdout.flush().await.unwrap();
                        let new_prompt = match outp.split("\n").last(){
                            Some(s)=>s,
                            None => ""
                        };
                        if prompt_tx.send(String::from(new_prompt)).err().is_some() {
                            continue;
                        }
                    }
                    _ = listener::wait_for_signal(tx_copy.subscribe(), "quit", &mut raw_mode) =>{
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
            loop {
                let stdout = stdout().into_raw_mode().unwrap();
                if !raw_mode {
                    stdout.suspend_raw_mode().unwrap();
                    let new_prompt = prompt_rx.borrow_and_update().to_owned();
                    let mut content = read_line(rl.clone(), Some(new_prompt.as_str())).await;

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
                        Key::Esc => {
                            handle_to_soc_send
                                .send(String::from_utf8_lossy(&([0x1b] as [u8; 1])).into_owned())
                                .await
                                .unwrap();
                        }
                        Key::Delete | Key::Backspace => {
                            handle_to_soc_send
                                .send(String::from_utf8_lossy(&([0x7f] as [u8; 1])).into_owned())
                                .await
                                .unwrap();
                        }
                        Key::Char('\n') | Key::Char('\r') => {
                            handle_to_soc_send
                                .send(String::from_utf8_lossy(&([0x0d] as [u8; 1])).into_owned())
                                .await
                                .unwrap();
                        }
                        Key::Up | Key::Left | Key::Right | Key::Down => {
                            let dir_key = match key {
                                Key::Up => 0x41,
                                Key::Left => 0x44,
                                Key::Right => 0x43,
                                Key::Down => 0x42,
                                _ => continue,
                            };
                            handle_to_soc_send
                                .send(
                                    String::from_utf8_lossy(&([0x1b, 0x4f, dir_key] as [u8; 3]))
                                        .into_owned(),
                                )
                                .await
                                .unwrap();
                        }
                        Key::Alt(ch) => {
                            handle_to_soc_send
                                .send(
                                    String::from_utf8_lossy(&([0x1b, ch as u8] as [u8; 2]))
                                        .into_owned(),
                                )
                                .await
                                .unwrap();
                        }
                        Key::Ctrl(ch) => {
                            let low_ch = ch.to_ascii_lowercase();
                            if low_ch as u8 > 96 {
                                let ctrl_digit = low_ch as u8 - 96;
                                handle_to_soc_send
                                    .send(
                                        String::from_utf8_lossy(&([ctrl_digit] as [u8; 1]))
                                            .into_owned(),
                                    )
                                    .await
                                    .unwrap();
                            }
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
