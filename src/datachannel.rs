use std::sync::Arc;

enum ReceiveState {
    Receiver(tokio::sync::mpsc::Receiver<Vec<u8>>),
    Closed,
}

pub struct DataChannel {
    dc: std::sync::Arc<webrtc::data_channel::RTCDataChannel>,
    receive_state: tokio::sync::Mutex<ReceiveState>,
}

impl DataChannel {
    pub async fn new(
        dc: std::sync::Arc<webrtc::data_channel::RTCDataChannel>,
    ) -> std::sync::Arc<DataChannel> {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let sender = std::sync::Arc::new(sender);
        let dc2 = std::sync::Arc::new(DataChannel {
            dc,
            receive_state: tokio::sync::Mutex::new(ReceiveState::Receiver(receiver)),
        });
        {
            let dc2 = dc2.clone();
            dc2.dc
                .on_message(Box::new(move |msg| {
                    let sender = sender.clone();
                    Box::pin(async move {
                        sender
                            .send(msg.data.to_vec())
                            .await
                            .expect("receive message");
                    })
                }))
                .await;
        }

        {
            let dc2 = dc2.clone();
            let dc3 = dc2.clone();
            dc2.dc
                .on_close(Box::new(move || {
                    let dc3 = dc3.clone();
                    Box::pin(async move {
                        *dc3.receive_state.lock().await = ReceiveState::Closed;
                    })
                }))
                .await;
        }
        let notify = Arc::new(tokio::sync::Notify::new());
        {
            let dc2 = dc2.clone();
            let notify = notify.clone();
            dc2.dc
                .on_open(Box::new(move || {
                    let notify = notify.clone();
                    Box::pin(async move {
                        notify.notify_one();
                    })
                }))
                .await;
        }
        notify.notified().await;
        dc2
    }

    pub async fn send(&self, data: &[u8]) -> Result<usize, webrtc::Error> {
        self.dc
            .send(&prost::bytes::Bytes::copy_from_slice(data))
            .await
    }

    pub async fn receive(&self) -> Option<Vec<u8>> {
        match &mut *self.receive_state.lock().await {
            ReceiveState::Closed => None,
            ReceiveState::Receiver(receiver) => receiver.recv().await,
        }
    }

    pub async fn close(&self) -> Result<(), webrtc::Error> {
        self.dc.close().await
    }
}
