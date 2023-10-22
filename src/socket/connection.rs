use std::sync::Arc;

use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

pub struct Handle
{
    pub read_stream: Arc<Mutex<OwnedReadHalf>>,
    pub write_stream: Arc<Mutex<OwnedWriteHalf>>,
    pub raw_mode: bool,
}

impl Handle {
    pub fn new(read_stream: OwnedReadHalf, write_stream: OwnedWriteHalf) -> Handle{
        let handle = Handle {
            read_stream: Arc::new(Mutex::new(read_stream)),
            write_stream: Arc::new(Mutex::new(write_stream)),
            raw_mode: false,
        };
        return handle;
    }
}

impl Clone for Handle {
    fn clone(&self) -> Self {
        Handle {
            write_stream: self.write_stream.clone(),
            read_stream: self.read_stream.clone(),
            raw_mode: self.raw_mode,
        }
    }
}
