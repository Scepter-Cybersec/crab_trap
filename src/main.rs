use std::collections::HashMap;
use std::env::{self, set_current_dir};
use std::sync::Arc;
use std::time::Duration;

use menu::menu_list::clear;
use rustyline::DefaultEditor;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::time::sleep;

use crate::menu::menu_list;
use crate::socket::{connection, listener};
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use sha256::digest;
use std::io::Write;
use termion::{self, color, cursor};
use tokio::sync::{mpsc, watch, Mutex};

mod menu;
mod socket;

fn input_loop(
    shells: Arc<Mutex<HashMap<String, connection::Handle>>>,
    menu: menu_list::MenuList,
    mut menu_channel_acquire: mpsc::Receiver<()>,
    menu_channel_release: mpsc::Sender<()>,
    init_message: Option<String>,
) {
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        let (_, height) = termion::terminal_size().unwrap();
        let msg_start = height - (menu.len() as u16);
        stdout
            .write_all(format!("{start}", start = cursor::Goto(0, msg_start)).as_bytes())
            .await
            .unwrap();
        stdout.flush().await.unwrap_or_default();

        let mut menu_rl = DefaultEditor::new().unwrap();
        clear();
        if init_message.is_some() {
            let msg = init_message.unwrap();
            println!("{msg}\n");
        }

        menu_list::help();
        loop {
            let (_, height) = termion::terminal_size().unwrap();
            let mut pwd = match env::current_dir() {
                Ok(path) => String::from(path.to_str().unwrap_or("")),
                Err(_) => String::from(""),
            };
            let home = match dirs::home_dir() {
                Some(p) => String::from(p.to_str().unwrap_or("")),
                None => String::from(""),
            };
            pwd = pwd.replace(&home, "~");
            let prompt = format!(
                "{bottom}{red}crab_trap 🦀:{pwd} #{reset} ",
                bottom = cursor::Goto(0, height),
                red = color::Fg(color::LightRed),
                reset = color::Fg(color::Reset)
            );

            let content = match menu_rl.readline(prompt.as_str()) {
                Ok(line) => {
                    menu_rl.add_history_entry(line.as_str()).unwrap_or_default();
                    line
                }
                Err(_) => continue,
            };

            let key = String::from(content.trim_end());

            let entry = match menu.get(&*key) {
                Some(val) => val,
                None => {
                    // need to handle cd differently
                    if key.starts_with("cd ") {
                        let mut dir = String::from(key.split(" ").last().unwrap_or_default());
                        if dir.len() <= 0 {
                            continue;
                        }
                        dir = dir.replace("~", &home);
                        let err = set_current_dir(dir).err();
                        if err.is_some() {
                            let display_err = err.unwrap();
                            println!("error changing directories: {display_err}");
                        }
                    } else {
                        let output = if cfg!(target_os = "windows") {
                            Command::new("cmd")
                                .args(["/C", key.as_str()])
                                .output()
                                .await
                                .unwrap()
                        } else {
                            Command::new("sh")
                                .arg("-c")
                                .arg(key.as_str())
                                .output()
                                .await
                                .unwrap()
                        };

                        let out = output.stdout;
                        let err = output.stderr;
                        std::io::stdout().write_all(&out).unwrap();
                        std::io::stderr().write_all(&err).unwrap();
                    }
                    continue;
                }
            };

            entry(shells.clone(), menu_channel_release.clone());

            if !key.eq(&String::new()) {
                if key.eq("l") {
                    menu_channel_acquire.recv().await.unwrap();
                }
            }
        }
    });
}

async fn soc_is_shell(soc: &mut TcpStream, soc_key: String) -> bool {
    soc.write(format!("echo {}\r\n", soc_key).as_bytes())
        .await
        .unwrap();
    let mut buf: [u8; 4096] = [0; 4096];
    for _ in 0..10 {
        let len = soc.try_read(&mut buf);
        if len.is_ok() {
            let content: String = String::from_utf8_lossy(&buf[..len.unwrap()]).into();
            if content.contains(&soc_key) {
                // add a new line so we get our prompt back
                soc.write("\n".as_bytes()).await.unwrap();
                return true;
            }
        }
        sleep(Duration::from_millis(200)).await;
    }
    return false;
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
    let (menu_channel_release, menu_channel_acquire) = mpsc::channel::<()>(1024);
    let menu = menu_list::new();

    // get user input
    let init_message = format!(
        "{red}listening on {bound_addr}:{bound_port}{reset}",
        red = color::Fg(color::LightRed),
        reset = color::Fg(color::Reset)
    );
    input_loop(
        connected_shells.clone(),
        menu,
        menu_channel_acquire,
        menu_channel_release.clone(),
        Some(init_message),
    );
    let socket_stream = listener::catch_sockets(bound_addr, bound_port);
    pin_mut!(socket_stream);

    let connected_shells_socket = connected_shells.clone();
    loop {
        let mut soc = socket_stream.next().await.unwrap().unwrap();
        let (handle_to_soc_send, handle_to_soc_recv) = mpsc::channel::<String>(1024);
        let (soc_to_handle_send, soc_to_handle_recv) = watch::channel::<String>(String::from(""));

        let (handle, _, soc_kill_sig_recv) = connection::Handle::new();
        let connected_shells_copy = connected_shells_socket.clone();

        let menu_release_copy = menu_channel_release.clone();
        tokio::spawn(async move {
            let soc_key: String;
            match &soc.peer_addr() {
                Ok(val) => {
                    soc_key = digest(val.to_string())[0..31].to_string();

                    let is_shell = soc_is_shell(&mut soc, soc_key.clone()).await;
                    if is_shell {
                        let mut shells = connected_shells_copy.lock().await;
                        listener::start_socket(
                            soc,
                            soc_to_handle_send,
                            handle_to_soc_recv,
                            soc_kill_sig_recv,
                        );
                        handle.handle_listen(
                            handle_to_soc_send,
                            soc_to_handle_recv,
                            menu_release_copy,
                        );
                        shells.insert(soc_key, handle);
                    } else {
                        return;
                    }
                }
                Err(_) => return,
            }
        });
    }
}
