use std::{io::stdin, sync::Arc};

use rustyline::{history::MemHistory, Editor};
use termion::{
    event::{Event, Key},
    input::TermReadEventsAndRaw,
};
use tokio::{
    sync::{
        oneshot::{self, error::RecvError},
        Mutex,
    },
    task,
};

pub async fn read_line(
    rl: Arc<Mutex<Editor<(), MemHistory>>>,
    prompt: Option<&str>,
) -> Result<String, RecvError> {
    let (tx, rx) = oneshot::channel::<String>();
    let input_prompt = match prompt {
        Some(s) => String::from(s),
        None => String::new(),
    };
    tokio::spawn(async move {
        let mut reader = rl.lock().await;

        let raw_content = reader.readline(&input_prompt);

        let content = match raw_content {
            Ok(line) => line,
            Err(_) => String::from(""),
        };
        reader.add_history_entry(content.clone()).unwrap();
        tx.send(content + "\n")
            .unwrap_or_else(|err| eprintln!("Error from readline handler: {err}"));
    });
    rx.await
}

pub async fn handle_key_input() -> Result<Option<(Key, Vec<u8>)>, RecvError> {
    let (tx, rx) = oneshot::channel::<Option<(Key, Vec<u8>)>>();
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
            .unwrap_or_else(|_| eprintln!("Error from raw readline handler"));
    });
    rx.await
}
