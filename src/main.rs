use std::collections::HashMap;
use std::env::{self, set_current_dir};
use std::sync::Arc;

use input::input::{read_line, CompletionHelper};
use menu::menu_list::clear;
use rustyline::history::MemHistory;
use rustyline::{Config, CompletionType, Editor};
use std::process::{exit, Command};
use termion::raw::IntoRawMode;

use crate::input::input::display_notification;
use crate::menu::menu_list;
use crate::socket::{connection, listener};
use connection::{handle_new_shell, Handle};
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
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
    shells: Arc<Mutex<HashMap<String, Handle>>>,
    menu: menu_list::MenuList,
    init_message: Option<String>,
) {
    tokio::spawn(async move {
        let history = MemHistory::new();
        let mut builder = Config::builder();
        builder = builder.completion_type(CompletionType::Circular);
        let config = builder.build();
        let mut rl = Editor::with_history(config, history).unwrap();
        let helper = CompletionHelper::new();
        rl.set_helper(Some(helper));
        let menu_rl = Arc::new(Mutex::new(rl));
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
            let content = match read_line(menu_rl.clone(), Some(&prompt)).await {
                Ok(line) => line,
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
    let connected_shells = Arc::new(Mutex::new(HashMap::<String, Handle>::new()));
    let menu = menu_list::new();

    // get user input
    let init_message = format!(
        "{red}listening on {bound_addr}:{bound_port}{reset}",
        red = color::Fg(color::LightRed),
        reset = color::Fg(color::Reset)
    );
    input_loop(connected_shells.clone(), menu, Some(init_message));
    let socket_stream = listener::catch_sockets(bound_addr.clone(), bound_port);
    pin_mut!(socket_stream);

    loop {
        let soc = match socket_stream.next().await.unwrap() {
            Ok(val) => val,
            Err(_) => {
                eprintln!("\nError address already in use {bound_addr}:{bound_port}");
                exit(1)
            }
        };

        let soc_added = handle_new_shell(soc, connected_shells.clone(), None).await;

        if soc_added {
            display_notification(String::from("new shell received!"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt() {
        let (prompt, home) = get_prompt();
        assert!(prompt
            .starts_with(format!("{red}crab_trap 🦀:", red = color::Fg(color::LightRed)).as_str()));
        assert_ne!(home, "");
    }
}
