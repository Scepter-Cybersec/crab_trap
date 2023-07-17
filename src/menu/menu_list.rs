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
    dyn Fn(MutexGuard<HashMap<String, connection::Handle>>, mpsc::Sender<()>)
        + Send
        + Sync
        + 'static,
>;

pub type MenuList = HashMap<&'static str, MenuListValue>;

pub fn help() {
    println!("l - list the connected shells");
    println!("h - display this help message");
    println!("clear - clear the display");
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

fn alias(
    shell_key: String,
    selected_index: u16,
    connected_shells: &mut MutexGuard<HashMap<String, connection::Handle>>,
) {
    let mut stdout = stdout();
    macro_rules! reset_alias_line {
        ($stdout:expr, $prompt:expr $(, $input:expr)?) => {
            write!(
                $stdout,
                "{goto}{prompt}{blink}{clear}",
                prompt = $prompt,
                goto = cursor::Goto(0, selected_index),
                clear = clear::AfterCursor,
                blink = cursor::BlinkingBlock
            )
            .unwrap();
            #[allow(unused_mut, unused_assignments)]
            let mut input = String::new();
            $ ( input = $input; )?
            if !input.is_empty(){
                write!(
                    $stdout,
                    "{input}{blink}{clear}",
                    input = input,
                    blink = cursor::BlinkingBlock,
                    clear = clear::AfterCursor
                ).unwrap();
            }
            $stdout.flush().unwrap();
        };
    }
    let mut prompt = String::from("Please enter a new alias: ");
    reset_alias_line!(stdout, prompt);
    let mut input = String::new();
    for key in stdin().keys() {
        match key.unwrap() {
            Key::Char(c) => {
                if c == '\n' || c == '\r' {
                    if input.is_empty() {
                        prompt = String::from("❌ Alias cannot be empty, please try again: ")
                    } else if connected_shells.contains_key(&input) {
                        input = String::new();
                        prompt = String::from("❌ Alias already exists, please try again: ")
                    } else {
                        let shell = connected_shells.remove(&shell_key).unwrap();
                        connected_shells.insert(input, shell);
                        return;
                    }
                } else {
                    input += &c.to_string();
                }
            }
            Key::Backspace | Key::Delete =>{
                if input.len() > 0{
                    input = String::from(&input[..input.len()-1]);
                }
            }
            Key::Esc => {
                return;
            }
            _ => {}
        }
        reset_alias_line!(stdout, prompt, input.clone());
        prompt = String::from("Please enter a new alias: ")
    }
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
            menu_esc_release.send(()).await.unwrap();
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
                menu_channel_release: mpsc::Sender<()>| {
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
                        Key::Delete | Key::Backspace => {
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
                        Key::Char('a') => {
                            let key: String =
                                connected_shells.clone().keys().collect::<Vec<&String>>()[cur_idx]
                                    .into();
                            alias(
                                key,
                                (((start_pos + cur_idx as u16) as i16 + line_offset)
                                    - (connected_shells.len() as i16))
                                    as u16,
                                &mut connected_shells,
                            );
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

    let clear = |_: MutexGuard<HashMap<String, connection::Handle>>, _: mpsc::Sender<()>| {
        clear();
    };

    menu.insert("clear", Box::new(clear));

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
