use std::{io::stdin, sync::Arc};

use futures_util::FutureExt;
use rustyline::{error::ReadlineError, history::MemHistory, Editor};
use termion::{
    event::{Event, Key},
    input::TermReadEventsAndRaw,
};
use tokio::{
    sync::{
        mpsc::{self, Sender},
        Mutex,
    },
    task,
};

pub async fn read_line(
    rl: Arc<Mutex<Editor<(), MemHistory>>>,
    tx: Sender<String>,
    prompt: Option<&str>,
) {
    let input_prompt = match prompt {
        Some(val) => String::from(val),
        None => String::from(""),
    };
    let tx_copy = tx.clone();
    let handle = tokio::spawn(async move {
        let mut reader = rl.lock().await;
        if tx_copy.is_closed() {
            return;
        }
        let raw_content = reader.readline(&input_prompt);

        let content = match raw_content {
            Ok(line) => line + "\n",
            Err(_) => String::from(""),
        };
        tx_copy
            .clone()
            .send(content)
            .await
            .unwrap_or_else(|err| eprintln!("{err}"));
    });
    tokio::spawn(async move {
        tx.closed()
            .then(|()| async move {
                handle.abort();
            })
            .await;
    });
}

pub async fn handle_key_input(tx: Sender<Option<(Key, Vec<u8>)>>) {
    if tx.is_closed() {
        return;
    }
    task::spawn(async move {
        let key_input = stdin().events_and_raw().next();
        let cleaned = match key_input {
            Some(key) => match key {
                Ok((Event::Key(k), raw)) => Some((k, raw)),
                Err(_) => None,
                _ => None,
            },

            None => None,
        };
        tx.send(cleaned)
            .await
            .unwrap_or_else(|err| eprintln!("{err}"));
    });
}
