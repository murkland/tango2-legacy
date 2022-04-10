#[derive(Clone, Debug)]
pub struct Input {
    pub local_tick: u32,
    pub remote_tick: u32,
    pub joyflags: u16,
    pub custom_screen_state: u8,
    pub turn: Option<[u8; 0x100]>,
}

pub struct Queue {
    semaphores: [std::sync::Arc<tokio::sync::Semaphore>; 2],
    queues: tokio::sync::Mutex<
        [std::collections::VecDeque<(Input, tokio::sync::OwnedSemaphorePermit)>; 2],
    >,
    local_player_index: u8,
    local_delay: u32,
}

impl Queue {
    pub fn new(size: usize, local_delay: u32, local_player_index: u8) -> Self {
        Queue {
            semaphores: [
                std::sync::Arc::new(tokio::sync::Semaphore::new(size)),
                std::sync::Arc::new(tokio::sync::Semaphore::new(size)),
            ],
            queues: tokio::sync::Mutex::new([
                std::collections::VecDeque::with_capacity(size),
                std::collections::VecDeque::with_capacity(size),
            ]),
            local_player_index,
            local_delay,
        }
    }

    pub async fn add_input(&mut self, player_index: u8, input: Input) {
        let sem = self.semaphores[player_index as usize].clone();
        let permit = sem.acquire_owned().await.unwrap();
        let mut queues = self.queues.lock().await;
        let queue = &mut queues[player_index as usize];
        queue.push_back((input, permit));
    }

    pub async fn queue_length(&self, player_index: u8) -> usize {
        let queues = self.queues.lock().await;
        queues[player_index as usize].len()
    }

    pub fn local_delay(&self) -> u32 {
        self.local_delay
    }

    pub async fn consume_and_peek_local(&mut self) -> (Vec<[Input; 2]>, Vec<Input>) {
        let mut queues = self.queues.lock().await;

        let to_commit = {
            let mut n =
                queues[self.local_player_index as usize].len() as isize - self.local_delay as isize;
            if (queues[(1 - self.local_player_index) as usize].len() as isize) < n {
                n = queues[(1 - self.local_player_index) as usize].len() as isize;
            }

            if n < 0 {
                vec![]
            } else {
                let [ref mut q0, ref mut q1] = &mut *queues;
                let p0 = q0.drain(..n as usize);
                let p1 = q1.drain(..n as usize);
                p0.zip(p1).map(|(i0, i1)| [i0, i1]).collect()
            }
        };

        let peeked = {
            let n =
                queues[self.local_player_index as usize].len() as isize - self.local_delay as isize;
            if n < 0 {
                vec![]
            } else {
                queues[self.local_player_index as usize]
                    .range(..n as usize)
                    .map(|(inp, _)| inp)
                    .cloned()
                    .collect()
            }
        };

        (
            to_commit
                .iter()
                .map(|[(inp1, _), (inp2, _)]| [inp1.clone(), inp2.clone()])
                .collect(),
            peeked,
        )
    }
}
