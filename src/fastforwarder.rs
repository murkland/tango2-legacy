use crate::hooks;
use crate::input;

struct InnerState {
    local_player_index: u8,
    input_pairs: std::collections::VecDeque<input::Pair<input::Input>>,
    commit_time: u32,
    committed_state: Option<mgba::state::State>,
    dirty_time: u32,
    dirty_state: Option<mgba::state::State>,
    result: anyhow::Result<()>,
}

pub struct Fastforwarder {
    core: mgba::core::Core,
    state: State,
    hooks: Box<dyn hooks::Hooks>,
    _trapper: mgba::trapper::Trapper,
}

#[derive(Clone)]
struct State(std::rc::Rc<std::cell::RefCell<Option<InnerState>>>);

impl hooks::FastforwarderState for State {
    fn commit_time(&self) -> u32 {
        self.0.borrow().as_ref().expect("commit time").commit_time
    }

    fn set_committed_state(&self, state: mgba::state::State) {
        self.0
            .borrow_mut()
            .as_mut()
            .expect("committed state")
            .committed_state = Some(state);
    }

    fn dirty_time(&self) -> u32 {
        self.0.borrow().as_ref().expect("dirty time").dirty_time
    }

    fn set_dirty_state(&self, state: mgba::state::State) {
        self.0
            .borrow_mut()
            .as_mut()
            .expect("dirty state")
            .dirty_state = Some(state);
    }

    fn peek_input_pair(&self) -> Option<input::Pair<input::Input>> {
        self.0
            .borrow()
            .as_ref()
            .expect("input pairs")
            .input_pairs
            .front()
            .cloned()
    }

    fn pop_input_pair(&self) -> Option<input::Pair<input::Input>> {
        self.0
            .borrow_mut()
            .as_mut()
            .expect("input pairs")
            .input_pairs
            .pop_front()
    }

    fn set_anyhow_error(&self, err: anyhow::Error) {
        self.0.borrow_mut().as_mut().expect("error").result = Err(err);
    }

    fn local_player_index(&self) -> u8 {
        self.0.borrow().as_ref().expect("error").local_player_index
    }
}

impl Fastforwarder {
    pub fn new(rom_path: &std::path::Path, hooks: Box<dyn hooks::Hooks>) -> anyhow::Result<Self> {
        let mut core = {
            let mut core = mgba::core::Core::new_gba("tango")?;
            let rom_vf = mgba::vfile::VFile::open(rom_path, mgba::vfile::flags::O_RDONLY)?;
            core.as_mut().load_rom(rom_vf)?;
            core
        };

        let state = State(std::rc::Rc::new(
            std::cell::RefCell::<Option<InnerState>>::new(None),
        ));

        let trapper = hooks.install_fastforwarder_hooks(core.as_mut(), Box::new(state.clone()));

        core.as_mut().reset();

        Ok(Fastforwarder {
            core,
            state,
            hooks,
            _trapper: trapper,
        })
    }

    pub fn fastforward(
        &mut self,
        state: &mgba::state::State,
        local_player_index: u8,
        commit_pairs: &[input::Pair<input::Input>],
        last_committed_remote_input: input::Input,
        local_player_inputs_left: &[input::Input],
    ) -> anyhow::Result<(
        mgba::state::State,
        mgba::state::State,
        input::Pair<input::Input>,
    )> {
        let input_pairs = commit_pairs
            .iter()
            .cloned()
            .chain(local_player_inputs_left.iter().cloned().map(|local| {
                let local_tick = local.local_tick;
                let remote_tick = local.remote_tick;
                input::Pair {
                    local,
                    remote: input::Input {
                        local_tick,
                        remote_tick,
                        joyflags: {
                            let mut joyflags = 0;
                            if last_committed_remote_input.joyflags & mgba::input::keys::A as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::A as u16;
                            }
                            if last_committed_remote_input.joyflags & mgba::input::keys::B as u16
                                != 0
                            {
                                joyflags |= mgba::input::keys::B as u16;
                            }
                            joyflags
                        },
                        custom_screen_state: last_committed_remote_input.custom_screen_state,
                        turn: vec![],
                    },
                }
            }))
            .collect::<std::collections::VecDeque<input::Pair<input::Input>>>();
        let last_input = input_pairs.back().expect("last input pair").clone();

        self.core.as_mut().load_state(state)?;
        self.hooks.prepare_for_fastforward(self.core.as_mut());

        let start_in_battle_time = self.hooks.in_battle_time(self.core.as_mut());
        let commit_time = start_in_battle_time + commit_pairs.len() as u32;
        let dirty_time = start_in_battle_time + input_pairs.len() as u32 - 1;

        *self.state.0.borrow_mut() = Some(InnerState {
            local_player_index,
            input_pairs,
            commit_time,
            committed_state: None,
            dirty_time,
            dirty_state: None,
            result: Ok(()),
        });

        while self
            .state
            .0
            .borrow()
            .as_ref()
            .unwrap()
            .committed_state
            .is_none()
            || self
                .state
                .0
                .borrow()
                .as_ref()
                .expect("state")
                .dirty_state
                .is_none()
        {
            self.state.0.borrow_mut().as_mut().expect("state").result = Ok(());
            self.core.as_mut().run_frame();
            if self
                .state
                .0
                .borrow()
                .as_ref()
                .expect("state")
                .result
                .is_err()
            {
                let state = self.state.0.take().expect("state");
                return Err(state.result.expect_err("state result err"));
            }
        }

        let state = self.state.0.take().expect("state");
        Ok((
            state.committed_state.expect("committed state"),
            state.dirty_state.expect("dirty state"),
            last_input,
        ))
    }
}
