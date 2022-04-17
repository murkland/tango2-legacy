#[derive(Clone, Debug)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Vec<u8>,
}

pub struct PairQueue<T>
where
    T: Clone,
{
    max_length: usize,
    queues: tokio::sync::Mutex<(std::collections::VecDeque<T>, std::collections::VecDeque<T>)>,
    local_delay: u32,
}

#[derive(Clone, Debug)]
pub struct Pair<T>
where
    T: Clone,
{
    pub local: T,
    pub remote: T,
}

impl<T> PairQueue<T>
where
    T: Clone,
{
    pub fn new(max_length: usize, local_delay: u32) -> Self {
        PairQueue {
            max_length,
            queues: tokio::sync::Mutex::new((
                std::collections::VecDeque::with_capacity(max_length),
                std::collections::VecDeque::with_capacity(max_length),
            )),
            local_delay,
        }
    }

    pub async fn add_local_input(&self, v: T) -> bool {
        let mut queues = self.queues.lock().await;
        if queues.0.len() >= self.max_length {
            return false;
        }
        queues.0.push_back(v);
        true
    }

    pub async fn add_remote_input(&self, v: T) -> bool {
        let mut queues = self.queues.lock().await;
        if queues.1.len() >= self.max_length {
            return false;
        }
        queues.1.push_back(v);
        true
    }

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub async fn local_queue_length(&self) -> usize {
        let queues = self.queues.lock().await;
        queues.0.len()
    }

    pub async fn remote_queue_length(&self) -> usize {
        let queues = self.queues.lock().await;
        queues.1.len()
    }

    pub async fn consume_and_peek_local(&mut self) -> (Vec<Pair<T>>, Vec<T>) {
        let mut queues = self.queues.lock().await;

        let to_commit = {
            let mut n = queues.0.len() as isize - self.local_delay as isize;
            if (queues.1.len() as isize) < n {
                n = queues.1.len() as isize;
            }

            if n < 0 {
                vec![]
            } else {
                let (ref mut localq, ref mut remoteq) = &mut *queues;
                let localxs = localq.drain(..n as usize);
                let remotexs = remoteq.drain(..n as usize);
                localxs
                    .zip(remotexs)
                    .map(|(local, remote)| Pair { local, remote })
                    .collect()
            }
        };

        let peeked = {
            let n = queues.0.len() as isize - self.local_delay as isize;
            if n < 0 {
                vec![]
            } else {
                queues.0.range(..n as usize).cloned().collect()
            }
        };

        (to_commit, peeked)
    }
}
