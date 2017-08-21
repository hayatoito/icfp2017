use punter::game::{Game, Strategy};
use punter::io::*;
use punter::prelude::*;
use punter::protocol::*;
use serde_json;
use std::io::{stdin, stdout};

pub fn offline_run() -> PunterResult<()> {
    let strategy = Strategy::EdgeWeight;
    let mut io = OfflineIO::new(stdin(), stdout());

    // 1. handshake
    let me = HandshakePS { me: "hayatox".to_string() };
    debug!("HandshakePS: {:?}", me);
    io.write_json_message(&serde_json::to_string(&me).unwrap())?;

    let s = io.read_json_message()?;
    let you: HandshakeSP = serde_json::from_str(&s)?;
    debug!("HandshakeSP: {:?}", you);

    // 2. dispatch by message type
    let s = io.read_json_message()?;
    let data: serde_json::Value = serde_json::from_str(&s)?;
    assert!(data.is_object());
    let o = data.as_object().unwrap();

    if let Some(_) = o.get("punter") {
        // setup
        let setup: SetupSP = serde_json::from_str(&s)?;
        debug!("Setup: {:?}", setup);
        let game: Game = setup.into();
        let setup = OfflineSetupPS {
            ready: game.me,
            // TODO: Support futures
            futures: None,
            state: serde_json::Value::String(game.encode()),
        };
        debug!("OfflineSetupPS: {:?}", setup);
        io.write_json_message(
            &serde_json::to_string(&setup).unwrap(),
        )?;
    } else if let Some(_) = o.get("move") {
        // game play
        let game_play: OfflineGamePlaySP = serde_json::from_str(&s)?;
        debug!("OfflineGamePlaySP: {:?}", game_play);
        let mut game: Game = game_play.into();
        let mov = game.play(strategy);
        game.apply_move(mov.clone());
        let game_play = mov.into_offline_game_play_ps(serde_json::Value::String(game.encode()));
        debug!("OfflineGamePlayPS: {:?}", game_play);
        io.write_json_message(
            &serde_json::to_string(&game_play).unwrap(),
        )?;
    } else if let Some(_) = o.get("stop") {
        let scoring: OfflineScoringSP = serde_json::from_str(&s)?;
        debug!("OfflineScoringSP: {:?}", scoring);
        info!("Scores: {:?}", scoring.stop.scores);

        // Re-use OfflineGamePlaySP to get the final game state.
        let game_play = OfflineGamePlaySP {
            moves: Moves { moves: scoring.stop.moves },
            state: scoring.state.into(),
        };
        let game: Game = game_play.into();
        debug!("final game state: {:?}", game);
    } else {
        unreachable!();
    }
    Ok(())
}

pub fn online_run(address: &str) -> PunterResult<Game> {
    let strategy = Strategy::EdgeWeight;
    let mut io = OnlineIO::new(address);

    // 1. handshake
    let me = HandshakePS { me: "hayatox".to_string() };
    debug!("HandshakePS: {:?}", me);
    io.write_json_message(&serde_json::to_string(&me).unwrap())?;

    let s = io.read_json_message()?;
    let you: HandshakeSP = serde_json::from_str(&s)?;
    debug!("HandshakeSP: {:?}", you);

    // 2. setup
    let s = io.read_json_message()?;
    let setup: SetupSP = serde_json::from_str(&s)?;
    debug!("SetupSP: {:?}", setup);

    let mut game: Game = setup.into();

    // Support futures
    // game.setup_futures();

    let setup_ps = OnlineSetupPS {
        ready: game.me,
        futures: game.convert_setup_futures_message(),
    };
    io.write_json_message(
        &serde_json::to_string(&setup_ps).unwrap(),
    )?;

    debug!("Game: {:?}", game);

    // 3. loop
    loop {
        let s = &io.read_json_message()?;
        let data: serde_json::Value = serde_json::from_str(s)?;
        assert!(data.is_object());
        let o = data.as_object().unwrap();
        if let Some(_) = o.get("move") {
            let game_play: OnlineGameplaySP = serde_json::from_str(s)?;
            game.apply_moves_excluding_me(game_play.move_.moves);
            let mov = game.play(strategy);
            game.apply_move(mov.clone());
            io.write_json_message(&serde_json::to_string(&mov)?)?;
        } else if let Some(_) = o.get("stop") {
            let scoring: OnlineScoringSP = serde_json::from_str(s)?;
            debug!("Scoring: {:?}", scoring);
            info!("Scores: {:?}", scoring.stop.scores);
            game.apply_moves_excluding_me(scoring.stop.moves);
            debug!("final game state: {:?}", game);
            break;
        } else {
            unreachable!();
        }
    }
    Ok(game)
}
