use std::collections::HashMap;

use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

use crate::socket::{connection, listener};
use futures_util::pin_mut;
use futures_util::stream::StreamExt;
use tokio::sync::mpsc;

mod socket;

#[tokio::main]
async fn main() {
    let (connected_shells_write, connected_shells_read) =
        watch::channel(HashMap::<String, connection::Handle>::new());

    let mut menu: HashMap<
        String,
        Box<dyn Fn(&mut HashMap<String, connection::Handle>, Option<String>) + Send + Sync + 'static>,
    > = HashMap::new();

    let list = |connected_shells: &mut HashMap<String, connection::Handle>, _: Option<String>| {
        for (key, _) in connected_shells.into_iter() {
            println!("{}", key);
        }
    };
    menu.insert(String::from("l"), Box::new(list));

    let start = |connected_shells: &mut HashMap<String, connection::Handle>,
                 key_opt: Option<String>| {
        if key_opt.is_some() {
            let key = key_opt.unwrap();
            let handle = match connected_shells.get(&key) {
                Some(val) => val,
                None => {
                    println!("Invalid session key!");
                    return;
                }
            };

            //start handler
            handle.egress.send("start").unwrap();
        } else {
            println!("Please provide a session key");
        }
    };

    menu.insert(String::from("s"), Box::new(start));

    let delete = |connected_shells: &mut HashMap<String, connection::Handle>,
                 key_opt: Option<String>| {
        if key_opt.is_some() {
            let key = key_opt.unwrap();
            let handle = match connected_shells.get(&key) {
                Some(val) => val,
                None => {
                    println!("Invalid session key!");
                    return;
                }
            };

            //delete handler
            handle.egress.send("delete").unwrap();
            connected_shells.remove(&key);
        } else {
            println!("Please provide a session key");
        }
    };

    menu.insert(String::from("d"), Box::new(delete));

    let help = |_: &mut HashMap<String, connection::Handle>, _: Option<String>| {
        println!("l - list the connected shells");
        println!("h - display this help message");
        println!("s <id> - start iteration with the specified reverse shell");
        println!("d <id> - remove the specified reverse shell");
    };
    menu.insert(String::from("h"), Box::new(help.clone()));

    // start main input listener
    let connected_shells_read_1 = connected_shells_read.clone();
    tokio::spawn(async move {
        let mut stdout = io::stdout();

        loop {
            stdout.write_all("crab_trap 🦀# ".as_bytes()).await.unwrap();
            stdout.flush().await.unwrap();
            let mut reader = BufReader::new(tokio::io::stdin());
            let mut buffer = Vec::new();
            reader.read_until(b'\n', &mut buffer).await.unwrap();
            let content = match String::from_utf8(buffer) {
                Ok(val) => val,
                Err(_) => continue,
            };
            let mut shells = connected_shells_read_1.borrow().clone();
            let clean_content = String::from(content.trim_end());
            let args = clean_content.split(" ").collect::<Vec<&str>>();

            let map_key = String::from(args[0]);
            let menu_arg: Option<String>;
            if args.len() > 1 {
                menu_arg = Some(String::from(args[1]));
            } else {
                menu_arg = None;
            }
            if !map_key.eq(&String::new()) {
                let menu_entry = menu.get(&map_key);
                match menu_entry {
                    Some(f) => f(&mut shells, menu_arg.clone()),
                    None => help(&mut shells, None),
                };
                let handle = shells.get(&menu_arg.unwrap_or(String::new()));
                if map_key.eq("s") && handle.is_some() {
                    loop {
                        match handle.unwrap().ingress.lock().await.recv().await {
                            Some("pause") => break,
                            Some(_) => continue,
                            None => continue,
                        }
                    }
                }
            }
        }
    });

    let socket_stream = listener::catch_sockets(String::from("127.0.0.1"), 8080);
    pin_mut!(socket_stream);

    let connected_shells_read_2 = connected_shells_read;
    loop {
        let soc = socket_stream.next().await.unwrap().unwrap();
        let (handle_to_soc_send, handle_to_soc_recv) = mpsc::channel::<String>(1024);
        let (soc_to_handle_send, mut soc_to_handle_recv) = mpsc::channel::<String>(1024);

        let (handle, handle_ingress_sender, handle_egress_receiver) = connection::Handle::new();
        match &soc.peer_addr() {
            Ok(val) => {
                let mut map = connected_shells_read_2.borrow().clone();
                map.insert(val.to_string(), handle);
                connected_shells_write.send(map).unwrap();
            }
            Err(_) => continue,
        }
        listener::start_socket(soc, soc_to_handle_send, handle_to_soc_recv);

        let mut handle_egress_receiver_1 = handle_egress_receiver.clone();

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
                    if listener::wait_for_signal(&mut handle_egress_receiver_1, "start")
                        .await
                        .is_err()
                    {
                        return;
                    }
                    // send a new line so we get a prompt when we return
                    content = String::from("\n");
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

                let changed = handle_egress_receiver_2.has_changed().unwrap();
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
