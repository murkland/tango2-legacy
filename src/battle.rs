pub struct Match {
    session_id: String,
    match_type: u16,
    game_title: String,
    game_crc32: u32,

    won_last_battle: bool,

    battle: parking_lot::Mutex<(u32, Option<Battle>)>,
    aborted: std::sync::atomic::AtomicBool,
}

impl Match {
    pub fn new(session_id: String, match_type: u16, game_title: String, game_crc32: u32) -> Self {
        Match {
            session_id,
            match_type,
            game_title,
            game_crc32,
            won_last_battle: false,
            battle: parking_lot::Mutex::new((0, None)),
            aborted: false.into(),
        }
    }

    pub fn abort(&mut self) {
        self.aborted
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn aborted(&mut self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn lock_battle(&self) -> parking_lot::MappedMutexGuard<Option<Battle>> {
        parking_lot::MutexGuard::map(self.battle.lock(), |(_, b)| b)
    }
}

pub struct Battle {}
