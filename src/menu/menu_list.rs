use std::cmp::max;
use std::collections::HashMap;

use connection::Handle;
use std::io::{stdin, stdout, Stdout, Write};
use std::sync::Arc;
use termion::cursor::DetectCursorPos;
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::{IntoRawMode, RawTerminal};
use termion::{clear, color, cursor, terminal_size};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex, MutexGuard};
use tokio::task::JoinHandle;
use tokio::{join, select};
use tokio_util::sync::CancellationToken;

use crate::input::input::{self, read_line};
use crate::socket::connection;

pub type MenuListValue = Box<
    dyn Fn(Arc<Mutex<HashMap<String, Handle>>>) -> Option<JoinHandle<()>> + Send + Sync + 'static,
>;

pub type MenuList = HashMap<&'static str, MenuListValue>;

pub fn help() {
    println!("l - list the connected shells");
    println!("h - display this help message");
    println!("clear - clear the display");
    println!("exit - quit the program");
}

pub fn clear() {
    println!("{clear}", clear = clear::All);
}

pub fn exit() {
    std::process::exit(0);
}

async fn soc_read<W>(handle: Handle, mut out_writer: W, cancel_token: CancellationToken)
where
    W: Write,
{
    let mut read_soc = handle.read_stream.lock().await;
    let mut read_buf: [u8; 4096] = [0; 4096];
    loop {
        let reader = read_soc.read(&mut read_buf);
        let cancel_fut = cancel_token.cancelled();
        select! {
            bytes_read = reader => {
                if bytes_read.is_err(){
                    eprint!("Channel closed");
                    cancel_token.cancel();
                    return
                }
                let n = bytes_read.unwrap();
                out_writer.write_all(&mut read_buf[0..n]).unwrap();
                out_writer.flush().unwrap();
            }
            _ = cancel_fut =>{
                break;
            }
        }
    }
}

async fn soc_write(handle: Handle, cancel_token: CancellationToken) {
    let mut write_soc = handle.write_stream.lock().await;
    loop {
        if handle.raw_mode {
            let out = stdout();
            let raw_stdout = out.into_raw_mode().unwrap();

            let cancel_fut = cancel_token.cancelled();
            let input_future = input::handle_key_input();
            select! {
                Ok(Some((key, key_bytes))) = input_future =>{
                    if key == Key::Ctrl('b'){
                        raw_stdout.suspend_raw_mode().unwrap();
                        cancel_token.cancel();
                        return
                    }
                    write_soc.write_all(&key_bytes).await.unwrap();
                    write_soc.flush().await.unwrap();
                }
                _ = cancel_fut =>{
                    break;
                }
            };
        } else {
            let cancel_fut = cancel_token.cancelled();
            let input_future = read_line(handle.readline.clone(), None);
            select! {
                res = input_future =>{
                    if res.is_err(){
                        println!("receiving input failed");
                        cancel_token.cancel();
                        break;
                    }
                    let inp_string = res.unwrap();
                    if inp_string.trim_end() == "back"{
                        cancel_token.cancel();
                        break;
                    }
                    write_soc.write_all(inp_string.as_bytes()).await.unwrap();
                    write_soc.flush().await.unwrap();
                }
                _ = cancel_fut =>{
                    break;
                }
            }
        }
    }
}

/// Sends the start signal to a handle, await until the handle is paused
async fn start(handle: Handle) {
    //start handler
    println!("{clear}", clear = clear::BeforeCursor);

    let quit_token = CancellationToken::new();

    match handle.raw_mode {
        true => {
            println!(
                "\r\n{guide}type \"CTRL + b\" to return to menu{reset}\r\n",
                guide = color::Fg(color::Red),
                reset = color::Fg(color::Reset)
            );
        }
        false => {
            println!(
                "\r\n{guide}type \"back\" to return to menu{reset}\r\n",
                guide = color::Fg(color::Red),
                reset = color::Fg(color::Reset)
            );
        }
    }

    {
        if handle.raw_mode {
            let size = termion::terminal_size();
            if size.is_ok() {
                let (cols, rows) = size.unwrap();
                let mut write_soc = handle.write_stream.lock().await;
                write_soc
                    .write_all(format!("\nstty rows {rows} cols {cols}\n").as_bytes())
                    .await
                    .unwrap();
                write_soc.flush().await.unwrap();
            }
        }
    }

    let out_writer: Box<dyn Write + Send> = match handle.raw_mode {
        true => {
            let tty_res = termion::get_tty();
            let mut out: Box<dyn Write + Send> = Box::new(stdout());
            if tty_res.is_ok() {
                out = Box::new(tty_res.unwrap());
            }
            out
        }
        false => Box::new(stdout()),
    };

    let reader_handle = soc_read(handle.clone(), out_writer, quit_token.clone());

    // start write to socket thread
    let writer_handle = soc_write(handle.clone(), quit_token.clone());
    join!(reader_handle, writer_handle);
}

async fn delete(key: String, connected_shells: &mut MutexGuard<'_, HashMap<String, Handle>>) {
    let handle = match connected_shells.remove(&key) {
        Some(val) => val,
        None => {
            println!("Invalid session key!");
            return;
        }
    };
    handle.write_stream.lock().await.flush().await.unwrap();
}

fn alias(
    shell_key: String,
    selected_index: u16,
    connected_shells: &mut MutexGuard<HashMap<String, Handle>>,
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
            Key::Backspace | Key::Delete => {
                if input.len() > 0 {
                    input = String::from(&input[..input.len() - 1]);
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
    () => {
        println!(
            "\r\n{show}{blink}{clear}",
            show = cursor::Show,
            blink = cursor::BlinkingBlock,
            clear = clear::AfterCursor
        );
    };
}

fn list_menu_help(stdout: &mut RawTerminal<Stdout>) {
    let (width, start_pos) = terminal_size().unwrap();
    let msgs = [
        String::from("(ENTER - start shell) (DEL | BACK - remove shell) (ESC - back to menu)"),
        String::from("(a - rename shell) (r - enter tty (raw) mode)"),
    ];
    for msg in msgs {
        let mut display_msg = msg.clone();
        if msg.len() > width.into() {
            let split = max(width - 3, 0).into();
            display_msg = String::from(&msg.as_str()[..split]);
            display_msg = display_msg + "...";
        }
        write!(
            stdout,
            "\r\n{goto}{select}{msg}{reset}",
            goto = cursor::Goto(0, start_pos),
            msg = display_msg,
            select = color::Bg(color::LightBlack),
            reset = color::Bg(color::Reset)
        )
        .unwrap();
        stdout.flush().unwrap();
    }
}

fn refresh_list_display(
    stdout: &mut RawTerminal<Stdout>,
    cur_idx: usize,
    keys: Vec<(String, Handle)>,
) {
    write!(
        stdout,
        "{goto}{clear}{clear_before}",
        goto = cursor::Goto(0, 2 as u16),
        clear = clear::AfterCursor,
        clear_before = clear::BeforeCursor
    )
    .unwrap();
    stdout.flush().unwrap();
    for (i, key) in keys.clone().into_iter().enumerate() {
        let raw_mode = match key.1.raw_mode {
            true => " (raw)",
            false => "",
        };
        let selection: String;
        if i == cur_idx {
            selection = format!(
                "{select}{key}{raw}{reset}{hide}",
                key = key.0,
                raw = raw_mode,
                select = color::Bg(color::Red),
                hide = cursor::Hide,
                reset = color::Bg(color::Reset),
            );
        } else {
            selection = format!(
                "{key}{raw}{hide}",
                key = key.0,
                raw = raw_mode,
                hide = cursor::Hide,
            );
        }
        write!(stdout, "{}", selection).unwrap();
        if i < &keys.len() - 1 {
            write!(stdout, "\r\n{clear}", clear = clear::AfterCursor).unwrap();
        }
        stdout.flush().unwrap();
    }
    list_menu_help(stdout);
}

pub fn new() -> MenuList {
    let mut menu: MenuList = HashMap::new();

    let list = |connected_shells: Arc<Mutex<HashMap<String, Handle>>>| -> Option<JoinHandle<()>> {
        Some(tokio::spawn(async move {
            let stdin = stdin();
            let mut stdout = stdout().into_raw_mode().unwrap();
            let mut shell_list: Vec<(String, Handle)>;
            {
                shell_list = connected_shells
                    .lock()
                    .await
                    .iter()
                    .map(|item| (item.0.to_owned(), item.1.to_owned()))
                    .collect::<Vec<(String, Handle)>>()
            }
            if shell_list.len() > 0 {
                let (_, start_pos) = stdout.cursor_pos().unwrap();
                let mut cur_idx = 0;
                let mut keys: Vec<String>;

                keys = shell_list.iter().map(|item| item.0.to_owned()).collect();
                refresh_list_display(&mut stdout, cur_idx, shell_list.to_owned());

                let mut shells = connected_shells.lock().await;
                for key in stdin.keys() {
                    stdout.activate_raw_mode().unwrap();
                    {
                        match key.unwrap() {
                            Key::Esc => {
                                unlock_menu!();
                                return;
                            }
                            Key::Up => {
                                if cur_idx > 0 {
                                    cur_idx -= 1;
                                }
                            }
                            Key::Down => {
                                if cur_idx < keys.len() - 1 {
                                    cur_idx += 1;
                                }
                            }
                            Key::Char('\n') | Key::Char('\r') => {
                                let key = keys[cur_idx].to_owned();
                                let handle_opt = shells.get(&key);
                                if handle_opt.is_some() {
                                    let handle = handle_opt.unwrap().clone();
                                    println!(
                                        "\r\n{show}{blink}{clear}",
                                        show = cursor::Show,
                                        blink = cursor::BlinkingBlock,
                                        clear = clear::AfterCursor
                                    );
                                    // drop the mutex guard so we're not holding and waiting
                                    // drop(shells);
                                    stdout.suspend_raw_mode().unwrap();
                                    start(handle).await;
                                }
                                return;
                            }
                            Key::Char('r') => {
                                // activate raw mode
                                let key: String = keys[cur_idx].to_owned();
                                let handle = match shells.get_mut(&key) {
                                    Some(han) => han,
                                    None => return,
                                };
                                handle.raw_mode = !handle.raw_mode;
                            }
                            Key::Delete | Key::Backspace => {
                                let key: String = keys[cur_idx].to_owned();
                                delete(key, &mut shells).await;
                                if cur_idx > 0 {
                                    cur_idx -= 1;
                                }
                                write!(stdout, "{}", clear::CurrentLine).unwrap();
                                stdout.flush().unwrap();

                                if shells.is_empty() {
                                    unlock_menu!();
                                    return;
                                }
                            }
                            Key::Char('a') => {
                                let key: String = keys[cur_idx].to_owned();

                                alias(
                                    key,
                                    (((start_pos + cur_idx as u16) as i16) - (shells.len() as i16))
                                        as u16,
                                    &mut shells,
                                );
                            }
                            _ => {}
                        }
                    }
                    shell_list = shells
                        .iter()
                        .map(|item| (item.0.to_owned(), item.1.to_owned()))
                        .collect();
                    keys = shell_list.iter().map(|item| item.0.to_owned()).collect();
                    refresh_list_display(&mut stdout, cur_idx, shell_list);
                }
            }
            unlock_menu!();
        }))
    };
    menu.insert("l", Box::new(list));

    let clear = |_| {
        clear();
        None
    };

    menu.insert("clear", Box::new(clear));

    menu.insert(
        "h",
        Box::new(|_| {
            help();
            None
        }),
    );

    menu.insert(
        "exit",
        Box::new(|_| {
            exit();
            None
        }),
    );

    return menu;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        net::{TcpListener, TcpStream},
        time::sleep,
    };

    use crate::socket::connection::handle_new_shell;

    use super::*;

    #[tokio::test]
    async fn test_read_soc() {
        let listener_res = TcpListener::bind("127.0.0.1:32425").await;
        assert!(listener_res.is_ok());
        let listener = listener_res.unwrap();
        let init_handle = tokio::spawn(async move {
            let (soc, _) = listener.accept().await.unwrap();
            let connected_shells = Arc::new(Mutex::new(HashMap::<String, Handle>::new()));
            handle_new_shell(soc, connected_shells.clone(), Some(true)).await;
            assert!(connected_shells.lock().await.len() > 0);
            let cancel_token = CancellationToken::new();
            let cancel_token_copy = cancel_token.clone();

            let mut buf: Vec<u8> = Vec::new();
            let handle = connected_shells
                .lock()
                .await
                .iter()
                .next()
                .unwrap()
                .1
                .to_owned();
            let handle_copy = handle.clone();
            let write_handle = tokio::spawn(async move {
                let mut write_half = handle.write_stream.lock().await;
                write_half.write("hello\n".as_bytes()).await.unwrap();
                write_half.flush().await.unwrap();
                sleep(Duration::from_millis(500)).await;
                cancel_token_copy.cancel();
            });
            soc_read(handle_copy, &mut buf, cancel_token).await;
            write_handle.await.unwrap();
        });
        TcpStream::connect("127.0.0.1:32425").await.unwrap();
        init_handle.await.unwrap();
    }
}
