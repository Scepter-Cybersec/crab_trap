use std::collections::HashMap;

use std::io::{stdin, stdout, Stdout, Write};
use termion::cursor::DetectCursorPos;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{clear, color, cursor};
use tokio::sync::{mpsc, MutexGuard};

use crate::socket::connection;

pub type MenuListValue = Box<
    dyn Fn(MutexGuard<HashMap<String, connection::Handle>>, mpsc::Sender<bool>)
        + Send
        + Sync
        + 'static,
>;

pub type MenuList = HashMap<&'static str, MenuListValue>;

pub fn help() {
    println!("l - list the connected shells");
    println!("h - display this help message");
    println!("clear - clear the display");
    // println!("s <id> - start iteration with the specified reverse shell");
    // println!("d <id> - remove the specified reverse shell");
    // println!("a <id> <alias> - change the id of a connection")
}

pub fn clear() {
    println!("{clear}", clear = clear::All);
}

fn start(key: String, connected_shells: HashMap<String, connection::Handle>) {
    let handle = match connected_shells.get(&key) {
        Some(val) => val,
        None => {
            println!("Invalid session key!");
            return;
        }
    };

    //start handler
    handle.egress.send("start").unwrap();
}

fn delete(key: String, connected_shells: &mut MutexGuard<HashMap<String, connection::Handle>>) {
    let handle = match connected_shells.get(&key) {
        Some(val) => val,
        None => {
            println!("Invalid session key!");
            return;
        }
    };
    //delete handler
    handle.egress.send("delete").unwrap();
    match connected_shells.remove(&key) {
        Some(val) => val.soc_kill_token.cancel(),
        None => return,
    };
}

macro_rules! unlock_menu {
    ($menu_channel_release:expr) => {
        println!(
            "\r\n{show}{blink}{clear}",
            show = cursor::Show,
            blink = cursor::BlinkingBlock,
            clear = clear::AfterCursor
        );
        let menu_esc_release = $menu_channel_release.clone();
        tokio::spawn(async move {
            menu_esc_release.send(true).await.unwrap();
            return;
        });
    };
}

// fn list_menu_help(
//     stdout: &mut RawTerminal<Stdout>,
//     println!()
// ){}

fn refresh_list_display(
    stdout: &mut RawTerminal<Stdout>,
    start_pos: u16,
    cur_idx: usize,
    keys: Vec<&String>,
) {
    write!(
        stdout,
        "{goto}{clear}",
        goto = cursor::Goto(0, start_pos),
        clear = clear::AfterCursor
    )
    .unwrap();
    stdout.flush().unwrap();
    for (i, key) in keys.clone().into_iter().enumerate() {
        let selection: String;
        if i == cur_idx {
            selection = format!(
                "{select}{key}{reset}{hide}",
                key = key,
                select = color::Bg(color::LightRed),
                hide = cursor::Hide,
                reset = color::Bg(color::Reset),
            );
        } else {
            selection = format!("{key}{hide}", key = key, hide = cursor::Hide,);
        }
        write!(stdout, "{}", selection).unwrap();
        if i < &keys.len() - 1 {
            write!(stdout, "\r\n{clear}", clear = clear::AfterCursor).unwrap();
        }
        stdout.flush().unwrap();
    }
    // select_item!(stdout, cur_idx, keys, true);
}

pub fn new() -> MenuList {
    let mut menu: MenuList = HashMap::new();

    let list = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                menu_channel_release: mpsc::Sender<bool>| {
        let stdin = stdin();
        let mut stdout = stdout().into_raw_mode().unwrap();
        if connected_shells.len() > 0 {
            let (_, start_pos) = stdout.cursor_pos().unwrap();
            let mut cur_idx = 0;
            // refresh shell list
            {
                let keys = connected_shells.keys().collect::<Vec<&String>>();
                refresh_list_display(&mut stdout, start_pos, cur_idx, keys);
            }
            let mut line_offset: i16 = 1;
            for key in stdin.keys() {
                {
                    match key.unwrap() {
                        Key::Esc => {
                            unlock_menu!(menu_channel_release);
                            return;
                        }
                        Key::Up => {
                            if cur_idx > 0 {
                                cur_idx -= 1;
                            }
                        }
                        Key::Down => {
                            if cur_idx < connected_shells.len() - 1 {
                                cur_idx += 1;
                            }
                        }
                        Key::Char('\n') | Key::Char('\r') => {
                            let key = connected_shells.clone().keys().collect::<Vec<&String>>()
                                [cur_idx]
                                .into();
                            start(key, connected_shells.clone());
                            println!(
                                "\r\n{show}{blink}",
                                show = cursor::Show,
                                blink = cursor::BlinkingBlock
                            );
                            return;
                        }
                        Key::Char('d') => {
                            let key: String =
                                connected_shells.clone().keys().collect::<Vec<&String>>()[cur_idx]
                                    .into();
                            delete(key.to_owned(), &mut connected_shells);
                            line_offset -= 1;
                            if cur_idx > 0 {
                                cur_idx -= 1;
                            }

                            if connected_shells.is_empty() {
                                unlock_menu!(menu_channel_release);
                                return;
                            }
                        }
                        _ => {}
                    }
                }

                let shells = connected_shells.clone();
                refresh_list_display(
                    &mut stdout,
                    ((start_pos as i16 + line_offset) - (shells.len() as i16)) as u16,
                    cur_idx,
                    connected_shells.clone().keys().collect::<Vec<&String>>(),
                );
            }
        }
        unlock_menu!(menu_channel_release);
    };
    menu.insert("l", Box::new(list));

    let clear = |_: MutexGuard<HashMap<String, connection::Handle>>, _: mpsc::Sender<bool>| {
        clear();
    };

    menu.insert("clear", Box::new(clear));

    // let start = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
    //              key_opt: Option<Vec<&str>>| {
    //     if key_opt.is_some() && key_opt.clone().unwrap().len() > 0 {
    //         let key = key_opt.unwrap()[0];
    //         let handle = match connected_shells.get(key) {
    //             Some(val) => val,
    //             None => {
    //                 println!("Invalid session key!");
    //                 return;
    //             }
    //         };

    //         //start handler
    //         handle.egress.send("start").unwrap();
    //     } else {
    //         println!("Please provide a session key");
    //     }
    // };

    // menu.insert("s", Box::new(start));

    // let delete = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
    //               key_opt: Option<Vec<&str>>| {
    //     if key_opt.is_some() && key_opt.clone().unwrap().len() > 0 {
    //         let key = key_opt.unwrap()[0];
    //         let handle = match connected_shells.get(key) {
    //             Some(val) => val,
    //             None => {
    //                 println!("Invalid session key!");
    //                 return;
    //             }
    //         };

    //         //delete handler
    //         handle.egress.send("delete").unwrap();
    //         match connected_shells.remove(key) {
    //             Some(val) => val.soc_kill_token.cancel(),
    //             None => return,
    //         };
    //     } else {
    //         println!("Please provide a session key");
    //     }
    // };

    // menu.insert("d", Box::new(delete));

    // let alias = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
    //              key_opts: Option<Vec<&str>>| {
    //     if key_opts.is_some() && key_opts.clone().unwrap().len() > 1 {
    //         let args = key_opts.unwrap();
    //         let ses_key = args[0];
    //         let alias = args[1];
    //         let handle = match connected_shells.remove(ses_key) {
    //             Some(val) => val,
    //             None => {
    //                 println!("Invalid session key!");
    //                 return;
    //             }
    //         };
    //         connected_shells.insert(String::from(alias), handle);
    //     } else {
    //         println!("Please provide a session key and an alias");
    //     }
    // };

    // menu.insert("a", Box::new(alias));

    menu.insert("h", Box::new(|_, _| help()));

    return menu;
}
