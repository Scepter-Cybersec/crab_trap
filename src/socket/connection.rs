use crate::listener;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::select;
use tokio::sync::mpsc::{
    self, Receiver as IngressReceiver, Receiver, Sender as IngressSender, Sender,
};
use tokio::sync::watch::{self, Receiver as EgressReceiver, Sender as EgressSender};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Handle {
    pub egress: Arc<EgressSender<&'static str>>,
    pub ingress: Arc<Mutex<IngressReceiver<&'static str>>>,
    pub soc_kill_token: CancellationToken,
}

impl Handle {
    pub fn new() -> (
        Handle,
        IngressSender<&'static str>,
        EgressReceiver<&'static str>,
        CancellationToken,
    ) {
        let (ingress_sender, ingress_receiver) = mpsc::channel::<&str>(1024);
        let (egress_sender, egress_receiver) = watch::channel::<&str>("");
        let soc_kill_token = CancellationToken::new();
        let soc_kill_token_listen = soc_kill_token.clone();
        let handle = Handle {
            egress: Arc::new(egress_sender),
            ingress: Arc::new(Mutex::new(ingress_receiver)),
            soc_kill_token,
        };
        return (
            handle,
            ingress_sender,
            egress_receiver,
            soc_kill_token_listen,
        );
    }
}

pub fn handle_listen(
    sender: IngressSender<&'static str>,
    receiver: EgressReceiver<&'static str>,
    handle_to_soc_send: Sender<String>,
    mut soc_to_handle_recv: Receiver<String>,
    menu_channel_release: Sender<()>,
) {
    let mut handle_egress_receiver_1 = receiver.clone();
    let menu_channel_release_1 = menu_channel_release.clone();
    // start writer
    tokio::spawn(async move {
        // wait for start signal
        if listener::wait_for_signal(&mut handle_egress_receiver_1, "start")
            .await
            .is_err()
        {
            return;
        }

        let mut reader = BufReader::new(tokio::io::stdin());
        loop {
            let mut buffer = Vec::new();
            let input_read_handle = reader.read_until(b'\n', &mut buffer);
            let soc_read_handle = soc_to_handle_recv.recv();
            select! {
                _ = input_read_handle => {
                    let mut content = match String::from_utf8(buffer) {
                        Ok(val) => val,
                        Err(_) => continue,
                    };
                    if content.trim_end().eq("quit") {
                        sender.send("pause").await.unwrap();
                        menu_channel_release_1.send(()).await.unwrap();

                        // send a new line so we get a prompt when we return
                        content = String::from("\n");
                        if listener::wait_for_signal(&mut handle_egress_receiver_1, "start")
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    if handle_to_soc_send.send(content).await.is_err() {
                        return;
                    }
                }
                val = soc_read_handle => {

                    let resp = match val {
                        Some(item) => item,
                        None => String::from("")
                    };
                    let mut stdout = io::stdout();
                    stdout.write_all(resp.as_bytes()).await.unwrap();
                    stdout.flush().await.unwrap();
                }
            }
        }
    });
}
