use std::collections::HashMap;

use tokio::sync::MutexGuard;

use crate::socket::connection;

pub type MenuListValue = Box<
    dyn Fn(MutexGuard<HashMap<String, connection::Handle>>, Option<String>) + Send + Sync + 'static,
>;

pub type MenuList = HashMap<String, MenuListValue>;

pub fn help() {
    println!("l - list the connected shells");
    println!("h - display this help message");
    println!("s <id> - start iteration with the specified reverse shell");
    println!("d <id> - remove the specified reverse shell");
}

pub fn new() -> MenuList {
    let mut menu: MenuList = HashMap::new();

    let list = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                _: Option<String>| {
        let shells = connected_shells.clone();
        for (key, _) in shells.into_iter() {
            println!("{}", key);
        }
    };
    menu.insert(String::from("l"), Box::new(list));

    let start = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
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

    let delete = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
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
            match connected_shells.remove(&key) {
                Some(val) => val.soc_kill_token.cancel(),
                None => return,
            };
        } else {
            println!("Please provide a session key");
        }
    };

    menu.insert(String::from("d"), Box::new(delete));

    menu.insert(String::from("h"), Box::new(|_, _| help()));

    return menu;
}
