use std::collections::HashMap;

use std::io::{stdin, stdout, Write};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::{color, cursor};
use tokio::sync::{MutexGuard, mpsc};

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
    println!("{clear}", clear = termion::clear::All);
}

fn start(key: String, connected_shells: HashMap<String, connection::Handle>){
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


macro_rules! deselect_item {
    ($stdout:expr,$idx:expr,$items:expr) => {
        let (_, height) = termion::terminal_size().unwrap();
        let cur_display_idx = height - ($items.len() - $idx - 1) as u16;
        write!(
            $stdout,
            "{goto}{key}{reset}",
            goto = cursor::Goto(0, cur_display_idx as u16),
            key = *$items[$idx],
            reset = color::Bg(color::Reset),
        )
        .unwrap();
        $stdout.flush().unwrap();
    };
}

macro_rules! select_item {
    ($stdout:expr,$idx:expr,$items:expr) => {
        let (_, height) = termion::terminal_size().unwrap();
        let cur_display_idx = height - ($items.len() - $idx - 1) as u16;
        write!(
            $stdout,
            "{goto}{select}{key}{reset}",
            goto = cursor::Goto(0, cur_display_idx as u16),
            select = color::Bg(color::LightRed),
            key = *$items[$idx],
            reset = color::Bg(color::Reset),
        )
        .unwrap();
        $stdout.flush().unwrap();
    };
}

pub fn new() -> MenuList {
    let mut menu: MenuList = HashMap::new();

    let list = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                menu_channel_release: mpsc::Sender<bool>| {
        let shells = connected_shells.clone();
        let stdin = stdin();
        let mut stdout = stdout().into_raw_mode().unwrap();
        for (i, (key, _)) in shells.clone().into_iter().enumerate() {
            write!(stdout, "{}{hide}", key, hide = cursor::Hide).unwrap();
            if i < shells.len() - 1 {
                write!(stdout, "\r\n").unwrap();
            }
            stdout.flush().unwrap();
        }
        if shells.len() > 0 {
            let mut cur_idx = 0;
            let keys = shells.keys().collect::<Vec<&String>>();
            select_item!(stdout, cur_idx, keys);
            for key in stdin.keys() {
                match key.unwrap() {
                    Key::Esc => {
                        println!(
                            "\r\n{show}{blink}",
                            show = cursor::Show,
                            blink = cursor::BlinkingBlock
                        );
                        let menu_esc_release = menu_channel_release.clone();
                        tokio::spawn(async move {
                            menu_esc_release.clone().send(true).await.unwrap();
                            return
                        });
                        return;
                    }
                    Key::Up => {
                        if cur_idx > 0{
                            deselect_item!(stdout, cur_idx, keys);
                            cur_idx -= 1;
                            select_item!(stdout, cur_idx, keys);
                        }
                    },
                    Key::Down => {
                        if cur_idx < keys.len() - 1{
                            deselect_item!(stdout, cur_idx, keys);
                            cur_idx += 1;
                            select_item!(stdout, cur_idx, keys);
                        }
                    },
                    Key::Char('\n') =>{
                        let key = keys[cur_idx].into();
                        start(key, shells.clone());
                        println!(
                            "\r\n{show}{blink}",
                            show = cursor::Show,
                            blink = cursor::BlinkingBlock
                        );
                        return;
                    }
                    _ => {}
                }
            }
        }
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
