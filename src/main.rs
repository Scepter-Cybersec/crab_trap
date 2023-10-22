use std::collections::HashMap;
use std::env::{self, set_current_dir};
use std::sync::Arc;
use std::time::Duration;

use menu::menu_list::clear;
use rustyline::DefaultEditor;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use std::process::Command;
use termion::raw::IntoRawMode;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::sleep;

use crate::menu::menu_list;
use crate::socket::{connection, listener};
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use sha256::digest;
use std::io::stdout;
use termion::{self, color};
use tokio::sync::Mutex;

mod input;
mod menu;
mod socket;

fn get_prompt() -> (String, String) {
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
        "{red}crab_trap 🦀:{pwd} #{reset} ",
        red = color::Fg(color::LightRed),
        reset = color::Fg(color::Reset)
    );
    return (prompt, home);
}

fn input_loop(
    shells: Arc<Mutex<HashMap<String, connection::Handle>>>,
    menu: menu_list::MenuList,
    init_message: Option<String>,
) {
    tokio::spawn(async move {
        let mut menu_rl = DefaultEditor::new().unwrap();
        clear();
        if init_message.is_some() {
            let msg = init_message.unwrap();
            println!("{msg}\n");
        }

        menu_list::help();
        loop {
            let stdout = stdout().into_raw_mode().unwrap();
            stdout.suspend_raw_mode().unwrap();
            let (prompt, home) = get_prompt();
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
                        Command::new("sh")
                            .arg("-c")
                            .arg(key)
                            .spawn()
                            .expect("Failed to run command")
                            .wait_with_output()
                            .unwrap();
                    }
                    continue;
                }
            };

            let handle_opt = entry(shells.clone());
            if handle_opt.is_some() {
                let join_handle = handle_opt.unwrap();
                join_handle.await.unwrap_or_default();
            }
        }
    });
}

async fn soc_is_shell(read_stream: Arc<Mutex<OwnedReadHalf>>, write_stream:Arc<Mutex<OwnedWriteHalf>>, soc_key: String) -> bool {
    let read_soc = read_stream.lock().await;
    let mut write_soc = write_stream.lock().await;
    write_soc.write(format!("echo {}\r\n", soc_key).as_bytes())
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

async fn handle_new_shell(
    soc: TcpStream,
    connected_shells: Arc<
        Mutex<HashMap<String, connection::Handle>>,
    >,
    skip_validation: Option<bool>,
) {
    let soc_addr = soc.peer_addr();
    let (soc_read, soc_write) = soc.into_split();
    let handle = connection::Handle::new(soc_read, soc_write);

    tokio::spawn(async move {
        let soc_key: String;
        match soc_addr {
            Ok(val) => {
                soc_key = digest(val.to_string())[0..31].to_string();
                let is_shell = match skip_validation {
                    Some(true) => true,
                    _ => soc_is_shell(handle.read_stream.clone(), handle.write_stream.clone(), soc_key.clone()).await,
                };
                if is_shell {
                    let mut shells = connected_shells.lock().await;
                    shells.insert(soc_key, handle);
                } else {
                    return;
                }
            }
            Err(_) => return,
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
    let connected_shells = Arc::new(Mutex::new(
        HashMap::<String, connection::Handle>::new(),
    ));
    let menu = menu_list::new();

    // get user input
    let init_message = format!(
        "{red}listening on {bound_addr}:{bound_port}{reset}",
        red = color::Fg(color::LightRed),
        reset = color::Fg(color::Reset)
    );
    input_loop(connected_shells.clone(), menu, Some(init_message));
    let socket_stream = listener::catch_sockets(bound_addr, bound_port);
    pin_mut!(socket_stream);

    loop {
        let soc = socket_stream.next().await.unwrap().unwrap();
        handle_new_shell(soc, connected_shells.clone(), None).await;
    }
}

#[cfg(test)]
mod tests {
    // use tokio::net::TcpListener;

    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn test_prompt() {
        let (prompt, home) = get_prompt();
        assert!(prompt
            .starts_with(format!("{red}crab_trap 🦀:", red = color::Fg(color::LightRed)).as_str()));
        assert_ne!(home, "");
    }

    #[tokio::test]
    async fn test_soc_is_shell() {
        let listener_res = TcpListener::bind("127.0.0.1:32423").await;
        assert!(listener_res.is_ok());
        let listener = listener_res.unwrap();
        tokio::spawn(async move {
            let (mut soc, _) = listener.accept().await.unwrap();
            soc.write("test123\n".as_bytes()).await.unwrap();
        });
        let (read, write) = TcpStream::connect("127.0.0.1:32423").await.unwrap().into_split();
        let read_half = Arc::new(Mutex::new(read));
        let write_half = Arc::new(Mutex::new(write));
        assert!(soc_is_shell(read_half, write_half, String::from("test123")).await);
    }

    #[tokio::test]
    async fn test_handle_new_shell() {
        let listener_res = TcpListener::bind("127.0.0.1:32424").await;
        assert!(listener_res.is_ok());
        let listener = listener_res.unwrap();
        tokio::spawn(async move {
            let (soc, _) = listener.accept().await.unwrap();
            let connected_shells =
                Arc::new(Mutex::new(HashMap::<String, connection::Handle>::new()));
            handle_new_shell(
                soc,
                connected_shells.clone(),
                Some(true),
            )
            .await;
            assert!(connected_shells.lock().await.len() > 0);
        });
        TcpStream::connect("127.0.0.1:32424").await.unwrap();
    }
}
