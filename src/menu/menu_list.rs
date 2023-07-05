use std::collections::HashMap;

use tokio::sync::MutexGuard;

use crate::socket::connection;

pub type MenuListValue = Box<
    dyn Fn(MutexGuard<HashMap<String, connection::Handle>>, Option<Vec<&str>>)
        + Send
        + Sync
        + 'static,
>;

pub type MenuList = HashMap<&'static str, MenuListValue>;

pub fn help() {
    println!("l - list the connected shells");
    println!("h - display this help message");
    println!("s <id> - start iteration with the specified reverse shell");
    println!("d <id> - remove the specified reverse shell");
    println!("a <id> <alias> - change the id of a connection")
}

pub fn new() -> MenuList {
    let mut menu: MenuList = HashMap::new();

    let list = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                _: Option<Vec<&str>>| {
        let shells = connected_shells.clone();
        for (key, _) in shells.into_iter() {
            println!("{}", key);
        }
    };
    menu.insert("l", Box::new(list));

    let start = |connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                 key_opt: Option<Vec<&str>>| {
        if key_opt.is_some() && key_opt.clone().unwrap().len() > 0 {
            let key = key_opt.unwrap()[0];
            let handle = match connected_shells.get(key) {
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

    menu.insert("s", Box::new(start));

    let delete = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                  key_opt: Option<Vec<&str>>| {
        if key_opt.is_some() && key_opt.clone().unwrap().len() > 0 {
            let key = key_opt.unwrap()[0];
            let handle = match connected_shells.get(key) {
                Some(val) => val,
                None => {
                    println!("Invalid session key!");
                    return;
                }
            };

            //delete handler
            handle.egress.send("delete").unwrap();
            match connected_shells.remove(key) {
                Some(val) => val.soc_kill_token.cancel(),
                None => return,
            };
        } else {
            println!("Please provide a session key");
        }
    };

    menu.insert("d", Box::new(delete));

    let alias = |mut connected_shells: MutexGuard<HashMap<String, connection::Handle>>,
                 key_opts: Option<Vec<&str>>| {
        if key_opts.is_some() && key_opts.clone().unwrap().len() > 1 {
            let args = key_opts.unwrap();
            let ses_key = args[0];
            let alias = args[1];
            let handle = match connected_shells.remove(ses_key) {
                Some(val) => val,
                None => {
                    println!("Invalid session key!");
                    return;
                }
            };
            connected_shells.insert(String::from(alias), handle);
        }else{
            println!("Please provide a session key and an alias");
        }
    };

    menu.insert("a", Box::new(alias));

    menu.insert("h", Box::new(|_, _| help()));

    return menu;
}
