use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver as IngressReceiver, Sender as IngressSender};
use tokio::sync::watch::{self, Sender as EgressSender, Receiver as EgressReceiver};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct Handle{
    pub egress: Arc<EgressSender<&'static str>>,
    pub ingress: Arc<Mutex<IngressReceiver<&'static str>>>,
    pub soc_kill_token: CancellationToken
}

impl Handle{
    pub fn new() -> (Handle, IngressSender<&'static str>, EgressReceiver<&'static str>, CancellationToken) {
        let (ingress_sender,ingress_receiver ) = mpsc::channel::<&str>(1024);
        let (egress_sender, egress_receiver ) = watch::channel::<&str>("");
        let soc_kill_token = CancellationToken::new();
        let soc_kill_token_listen = soc_kill_token.clone();
        let handle = Handle{egress: Arc::new(egress_sender), ingress: Arc::new(Mutex::new(ingress_receiver)), soc_kill_token};
        return (handle, ingress_sender, egress_receiver, soc_kill_token_listen)
    }
}