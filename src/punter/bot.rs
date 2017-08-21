use punter::game::{Game, Strategy};
use punter::io::ChildIO;
use punter::prelude::*;
use punter::protocol::*;
use serde_json;
use std;

pub trait Bot {
    fn name(&self) -> String;
    fn setup(&mut self, setup: SetupSP) -> PunterResult<OfflineSetupPS>;
    fn play(&mut self, gameplay: OfflineGamePlaySP) -> PunterResult<OfflineGamePlayPS>;
    fn stop(&mut self, scoring: OfflineScoringSP) -> PunterResult<()>;
}

#[derive(Debug)]
pub struct InternalBot {
    strategy: Strategy,
    game: Option<Game>,
}

impl InternalBot {
    pub fn new(strategy: Strategy) -> Self {
        Self {
            strategy,
            game: None,
        }
    }
}

impl Bot for InternalBot {
    fn name(&self) -> String {
        format!("{:?}", self.strategy)
    }
    fn setup(&mut self, setup: SetupSP) -> PunterResult<OfflineSetupPS> {
        self.game = Some(setup.into());
        Ok(OfflineSetupPS {
            ready: self.game.as_ref().unwrap().me,
            futures: None,
            // Dummy
            state: Default::default(),
        })
    }
    fn play(&mut self, gameplay: OfflineGamePlaySP) -> PunterResult<OfflineGamePlayPS> {
        let game: &mut Game = self.game.as_mut().unwrap();
        game.apply_moves_excluding_me(gameplay.moves.moves);
        let mov = game.play(self.strategy);
        game.apply_move(mov.clone());
        Ok(mov.into_offline_game_play_ps(Default::default()))
    }
    fn stop(&mut self, _: OfflineScoringSP) -> PunterResult<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct OfflineBot {
    program: std::path::PathBuf,
}

impl OfflineBot {
    pub fn new<P: AsRef<std::path::Path>>(program: P) -> Self {
        OfflineBot { program: program.as_ref().to_owned() }
    }

    fn handsheke(&self, io: &mut ChildIO) -> PunterResult<()> {
        let s = io.read_json_message()?;
        let me: HandshakePS = serde_json::from_str(&s)?;
        debug!("HandshakePS: {:?}", me);

        let you = HandshakeSP { you: me.me };
        io.write_json_message(&serde_json::to_string(&you).unwrap())?;
        Ok(())
    }
}

impl Bot for OfflineBot {
    fn name(&self) -> String {
        self.program.as_os_str().to_str().unwrap().to_string()
    }
    fn setup(&mut self, setup: SetupSP) -> PunterResult<OfflineSetupPS> {
        let mut io = ChildIO::new(&self.program);
        self.handsheke(&mut io)?;
        io.write_json_message(
            &serde_json::to_string(&setup).unwrap(),
        )?;
        let s = io.read_json_message()?;
        io.wait()?;
        Ok(serde_json::from_str(&s)?)
    }
    fn play(&mut self, gameplay: OfflineGamePlaySP) -> PunterResult<OfflineGamePlayPS> {
        let mut io = ChildIO::new(&self.program);
        self.handsheke(&mut io)?;
        io.write_json_message(
            &serde_json::to_string(&gameplay).unwrap(),
        )?;
        let s = io.read_json_message()?;
        let gameplay: OfflineGamePlayPS = serde_json::from_str(&s)?;
        io.wait()?;
        Ok(gameplay)
    }
    fn stop(&mut self, scoring: OfflineScoringSP) -> PunterResult<()> {
        let mut io = ChildIO::new(&self.program);
        io.write_json_message(
            &serde_json::to_string(&scoring).unwrap(),
        )?;
        io.wait()?;
        Ok(())
    }
}
