use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast::{self, Sender as SocKillSender, Receiver as SocKillReceiver};
use tokio::sync::mpsc::{self, Receiver as IngressReceiver, Sender as IngressSender};
use tokio::sync::watch::{self, Sender as EgressSender, Receiver as EgressReceiver};

#[derive(Clone, Debug)]
pub struct Handle{
    pub egress: Arc<EgressSender<&'static str>>,
    pub ingress: Arc<Mutex<IngressReceiver<&'static str>>>,
    pub soc_kill_sig_send: SocKillSender<bool>
}

impl Handle{
    pub fn new() -> (Handle, IngressSender<&'static str>, EgressReceiver<&'static str>, SocKillReceiver<bool>) {
        let (ingress_sender,ingress_receiver ) = mpsc::channel::<&str>(1024);
        let (egress_sender, egress_receiver ) = watch::channel::<&str>("");
        let (soc_kill_sig_send, soc_kill_sig_recv) = broadcast::channel::<bool>(1024);
        let handle = Handle{egress: Arc::new(egress_sender), ingress: Arc::new(Mutex::new(ingress_receiver)), soc_kill_sig_send};
        return (handle, ingress_sender, egress_receiver, soc_kill_sig_recv)
    }
}