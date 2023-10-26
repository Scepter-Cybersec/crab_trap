use std::{
    io::{stdin, stdout, Write},
    sync::Arc,
};

use rustyline::{
    completion::{Completer, FilenameCompleter, Pair},
    highlight::Highlighter,
    hint::HistoryHinter,
    history::History,
    validate::MatchingBracketValidator,
    Editor, Helper,
};
use rustyline_derive::{Helper, Hinter, Validator};
use termion::{
    clear, color, cursor,
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

#[derive(Helper, Hinter, Validator)]
pub struct InputHelper {
    completer: Option<FilenameCompleter>,
    #[rustyline(Validator)]
    validator: MatchingBracketValidator,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
}

impl Highlighter for InputHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        return format!(
            "{grey}{hint}{reset}",
            grey = color::Fg(color::Rgb(100, 100, 100)),
            reset = color::Fg(color::Reset)
        )
        .into();
    }
}

impl Completer for InputHelper {
    type Candidate = Pair;
    fn complete(
        &self, // FIXME should be `&mut self`
        line: &str,
        pos: usize,
        ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        return match &self.completer {
            Some(completer) => completer.complete(line, pos, ctx),
            None => Ok((0, Vec::new())),
        };
    }
}

impl InputHelper {
    pub fn new() -> InputHelper {
        let helper: InputHelper = InputHelper {
            completer: Some(FilenameCompleter::new()),
            hinter: HistoryHinter {},
            validator: MatchingBracketValidator::new(),
        };
        return helper;
    }
    pub fn new_only_hinter() -> InputHelper {
        let helper: InputHelper = InputHelper {
            completer: None,
            hinter: HistoryHinter {},
            validator: MatchingBracketValidator::new(),
        };
        return helper;
    }
}

pub fn display_notification(text: String) {
    let mut stdout = stdout();
    let notification = format!(
        "{goto}{clear}{success_bg}{success}{text}{reset}{reset_bg}",
        goto = cursor::Goto(1, 1),
        clear = clear::CurrentLine,
        success = color::Fg(color::Red),
        success_bg = color::Bg(color::Rgb(255, 200, 0)),
        reset = color::Fg(color::Reset),
        reset_bg = color::Bg(color::Reset),
    );

    // save cursor position
    stdout.write_all(&"\x1B7".as_bytes()).unwrap();
    stdout.flush().unwrap();

    stdout.write_all(&notification.as_bytes()).unwrap();
    stdout.flush().unwrap();

    // restore cursor position
    stdout.write_all(&"\x1B8".as_bytes()).unwrap();
    stdout.flush().unwrap();
}

pub async fn read_line<T, H>(
    rl: Arc<Mutex<Editor<T, H>>>,
    prompt: Option<&str>,
) -> Result<String, RecvError>
where
    H: History + Send + 'static,
    T: Helper + Send + 'static,
{
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
