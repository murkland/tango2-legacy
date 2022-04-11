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
    local_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    remote_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    queues: tokio::sync::Mutex<(
        std::collections::VecDeque<(T, tokio::sync::OwnedSemaphorePermit)>,
        std::collections::VecDeque<(T, tokio::sync::OwnedSemaphorePermit)>,
    )>,
    local_delay: u32,
}

#[derive(Clone)]
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
    pub fn new(size: usize, local_delay: u32) -> Self {
        PairQueue {
            local_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(size)),
            remote_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(size)),
            queues: tokio::sync::Mutex::new((
                std::collections::VecDeque::with_capacity(size),
                std::collections::VecDeque::with_capacity(size),
            )),
            local_delay,
        }
    }

    pub async fn add_local_input(&self, v: T) {
        let sem = self.local_semaphore.clone();
        let permit = sem.acquire_owned().await.expect("acquire semaphore permit");
        let mut queues = self.queues.lock().await;
        queues.0.push_back((v, permit));
    }

    pub async fn add_remote_input(&self, v: T) {
        let sem = self.remote_semaphore.clone();
        let permit = sem.acquire_owned().await.expect("acquire semaphore permit");
        let mut queues = self.queues.lock().await;
        queues.1.push_back((v, permit));
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
                    .map(|((local, _), (remote, _))| Pair { local, remote })
                    .collect()
            }
        };

        let peeked = {
            let n = queues.0.len() as isize - self.local_delay as isize;
            if n < 0 {
                vec![]
            } else {
                queues
                    .0
                    .range(..n as usize)
                    .map(|(inp, _)| inp)
                    .cloned()
                    .collect()
            }
        };

        (to_commit, peeked)
    }
}
