use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use menu::menu_list::clear;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::menu::menu_list;
use crate::socket::{connection, listener};
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use tokio::sync::{mpsc, Mutex};
use sha256::{digest};
use termion::{self, color, cursor};

mod menu;
mod socket;

fn input_loop(shells:  Arc<Mutex<HashMap<String, connection::Handle>>>, menu: menu_list::MenuList, mut menu_channel_acquire: mpsc::Receiver<bool>, menu_channel_release: mpsc::Sender<bool>){
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        clear();
        let (_, height) = termion::terminal_size().unwrap();
        let msg_start = height - (menu.len() as u16);
        stdout.write_all(format!("{start}", start = cursor::Goto(0, msg_start)).as_bytes()).await.unwrap();
        stdout.flush().await.unwrap();
        menu_list::help();
        loop {
            let (_, height) = termion::terminal_size().unwrap();

            let prompt = format!("{bottom}{red}crab_trap 🦀#{reset} ",bottom = cursor::Goto(0,height), red = color::Fg(color::LightRed), reset = color::Fg(color::Reset));
            stdout.write_all(prompt.as_bytes()).await.unwrap();
            stdout.flush().await.unwrap();
            let mut reader = BufReader::new(tokio::io::stdin());
            let mut buffer = Vec::new();
            reader.read_until(b'\n', &mut buffer).await.unwrap();
            let content = match String::from_utf8(buffer) {
                Ok(val) => val,
                Err(_) => continue,
            };
            let key = String::from(content.trim_end());

            let entry = match menu.get(&*key) {
                Some(val) => val,
                None => continue
            };

            {
                let locked_shells = shells.lock().await;
                entry(locked_shells, menu_channel_release.clone())
            }

            if !key.eq(&String::new()) {
                if key.eq("l") {
                    loop {
                        match menu_channel_acquire.recv().await.unwrap_or(true){
                            true => break,
                            false => continue
                        }
                    }
                }
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let bound_addr: String;
    let bound_port: u16;
    if args.len() == 3 {
        bound_addr = args[1].clone();
        bound_port = match args[2].parse::<u16>() {
            Ok(val) => val,
            Err(_) => {
                println!("[-] Invalid port {}", args[2]);
                println!("Usage: {0} OR {0} <address> <port>", args[0]);
                return;
            }
        };
    } else if args.len() == 1 {
        bound_addr = String::from("0.0.0.0");
        bound_port = 4545;
    } else {
        println!("Usage: {0} OR {0} <address> <port>", args[0]);
        return;
    }
    let connected_shells = Arc::new(Mutex::new(HashMap::<String, connection::Handle>::new()));
    let (menu_channel_release, menu_channel_acquire) = mpsc::channel::<bool>(1024);
    let menu = menu_list::new();
    
    // get user input
    input_loop(connected_shells.clone(), menu, menu_channel_acquire, menu_channel_release.clone());

    let socket_stream = listener::catch_sockets(bound_addr, bound_port);
    pin_mut!(socket_stream);

    let connected_shells_socket = connected_shells.clone();
    loop {
        let soc = socket_stream.next().await.unwrap().unwrap();
        let (handle_to_soc_send, handle_to_soc_recv) = mpsc::channel::<String>(1024);
        let (soc_to_handle_send, mut soc_to_handle_recv) = mpsc::channel::<String>(1024);

        let (handle, handle_ingress_sender, handle_egress_receiver, soc_kill_sig_recv) =
            connection::Handle::new();
        {
            let mut shells = connected_shells_socket.lock().await;
            let soc_key: String;
            match &soc.peer_addr() {
                Ok(val) => {
                    soc_key = digest(val.to_string())[0..31].to_string();
                    shells.insert(soc_key, handle);
                }
                Err(_) => continue,
            }
        }

        listener::start_socket(
            soc,
            soc_to_handle_send,
            handle_to_soc_recv,
            soc_kill_sig_recv,
        );

        let mut handle_egress_receiver_1 = handle_egress_receiver.clone();
        let menu_channel_release_1 = menu_channel_release.clone();
        // start writer
        tokio::spawn(async move {
            // wait for start signal
            if listener::wait_for_signal(&mut handle_egress_receiver_1, "start")
                .await
                .is_err()
            {
                return;
            }

            loop {
                let mut reader = BufReader::new(tokio::io::stdin());
                let mut buffer = Vec::new();
                reader.read_until(b'\n', &mut buffer).await.unwrap();
                let mut content = match String::from_utf8(buffer) {
                    Ok(val) => val,
                    Err(_) => continue,
                };
                if content.trim_end().eq("quit") {
                    handle_ingress_sender.send("pause").await.unwrap();
                    menu_channel_release_1.send(true).await.unwrap();

                    // send a new line so we get a prompt when we return
                    content = String::from("\n");
                    if listener::wait_for_signal(&mut handle_egress_receiver_1, "start")
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if handle_to_soc_send.send(content).await.is_err() {
                    return;
                }
            }
        });

        // start reader
        let mut handle_egress_receiver_2 = handle_egress_receiver;
        tokio::spawn(async move {
            // wait for start signal
            if listener::wait_for_signal(&mut handle_egress_receiver_2, "start")
                .await
                .is_err()
            {
                return;
            };

            let mut stdout = io::stdout();
            loop {
                stdout.flush().await.unwrap();

                let resp = match soc_to_handle_recv.recv().await {
                    Some(val) => val,
                    None => String::from(""),
                };

                let changed = match handle_egress_receiver_2.has_changed() {
                    Ok(val) => val,
                    Err(_) => return,
                };
                if changed {
                    let val = *handle_egress_receiver_2.borrow();
                    if val.eq("pause") {
                        if listener::wait_for_signal(&mut handle_egress_receiver_2, "start")
                            .await
                            .is_err()
                        {
                            return;
                        };
                        continue;
                    }
                }
                stdout.write_all(resp.as_bytes()).await.unwrap();
            }
        });
    }
}
