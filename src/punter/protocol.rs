use punter::prelude::*;
use serde_json;
use std;
use std::str;

// 4.2 Online mode
// 0. Handshake

#[derive(Debug, Serialize, Deserialize)]
pub struct HandshakePS {
    pub me: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HandshakeSP {
    pub you: String,
}

// 1. Setup

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SetupSP {
    pub punter: PunterId,
    pub punters: usize,
    pub map: Map,
    pub settings: Option<Settings>,
}

// Extnsions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub futures: Option<bool>,
    pub splurge: Option<bool>,
    pub options: Option<bool>,
}

impl std::default::Default for Settings {
    fn default() -> Self {
        Settings {
            futures: Default::default(),
            splurge: Default::default(),
            options: Default::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OnlineSetupPS {
    pub ready: PunterId,
    pub futures: Option<Vec<Future>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Future {
    pub source: SiteId,
    pub target: SiteId,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Map {
    pub sites: Vec<Site>,
    pub rivers: Vec<River>,
    pub mines: Vec<SiteId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Site {
    pub id: SiteId,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct River {
    pub source: SiteId,
    pub target: SiteId,
}

// 2. Gameplay

#[derive(Debug, Serialize, Deserialize)]
pub struct OnlineGameplaySP {
    #[serde(rename = "move")]
    pub move_: Moves,
}

#[allow(dead_code)]
pub type OnlineGameplayPS = Move;

#[derive(Debug, Serialize, Deserialize)]
pub struct Moves {
    pub moves: Vec<Move>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)] // To reuse Claim, Pass, Splurge, Option_ structures
pub enum Move {
    Claim { claim: Claim },
    Pass { pass: Pass },
    Splurge { splurge: Splurge },
    Option_ { option: Option_ },
}

impl Move {
    pub fn from(claim: Claim) -> Self {
        Move::Claim { claim }
    }
    pub fn claimed_by(&self, me: PunterId) -> bool {
        match *self {
            Move::Claim { ref claim } => claim.punter == me,
            Move::Splurge { ref splurge } => splurge.punter == me,
            Move::Option_ { ref option } => option.punter == me,
            Move::Pass { ref pass } => pass.punter == me,
        }
    }
}

// 3. Scoring
#[derive(Debug, Serialize, Deserialize)]
pub struct OnlineScoringSP {
    pub stop: Scores,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Scores {
    pub moves: Vec<Move>,
    pub scores: Vec<Score>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub punter: PunterId,
    pub score: i64,
}

// 4.3 Offline mode

// 1. Setup

#[derive(Debug, Serialize, Deserialize)]
pub struct OfflineSetupPS {
    pub ready: PunterId,
    pub futures: Option<Vec<Future>>,
    pub state: EncodedGameState,
}

// 2. Gameplay
#[derive(Debug, Serialize, Deserialize)]
pub struct OfflineGamePlaySP {
    #[serde(rename = "move")]
    pub moves: Moves,
    pub state: EncodedGameState,
}

pub type EncodedGameState = serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OfflineGamePlayPS {
    Claim {
        claim: Claim,
        state: EncodedGameState,
    },
    #[serde(rename = "pass")]
    Pass { pass: Pass, state: EncodedGameState },
    Splurge {
        splurge: Splurge,
        state: EncodedGameState,
    },
    Option_ {
        option: Option_,
        state: EncodedGameState,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub punter: PunterId,
    pub source: SiteId,
    pub target: SiteId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pass {
    pub punter: PunterId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Splurge {
    pub punter: PunterId,
    pub route: Vec<SiteId>,
}

pub type Option_ = Claim;

impl OfflineGamePlayPS {
    pub fn state(&self) -> EncodedGameState {
        match *self {
            OfflineGamePlayPS::Claim { ref state, .. } => state.clone(),
            OfflineGamePlayPS::Pass { ref state, .. } => state.clone(),
            OfflineGamePlayPS::Splurge { ref state, .. } => state.clone(),
            OfflineGamePlayPS::Option_ { ref state, .. } => state.clone(),
        }
    }
}

impl From<OfflineGamePlayPS> for Move {
    fn from(gameplay: OfflineGamePlayPS) -> Self {
        match gameplay {
            OfflineGamePlayPS::Claim { claim, .. } => Move::Claim { claim },
            OfflineGamePlayPS::Pass { pass, .. } => Move::Pass { pass },
            OfflineGamePlayPS::Splurge { splurge, .. } => Move::Splurge { splurge },
            OfflineGamePlayPS::Option_ { option, .. } => Move::Option_ { option },
        }
    }
}

impl Move {
    pub fn into_offline_game_play_ps(self, state: EncodedGameState) -> OfflineGamePlayPS {
        match self {
            Move::Claim { claim } => OfflineGamePlayPS::Claim { claim, state },
            Move::Pass { pass } => OfflineGamePlayPS::Pass { pass, state },
            Move::Splurge { splurge } => OfflineGamePlayPS::Splurge { splurge, state },
            Move::Option_ { option } => OfflineGamePlayPS::Option_ { option, state },
        }
    }
}

// 3. Scoring
#[derive(Debug, Serialize, Deserialize)]
pub struct OfflineScoringSP {
    pub stop: Scores,
    pub state: EncodedGameState,
}

// 4.4 Timeouts
#[derive(Debug, Serialize, Deserialize)]
pub struct TimeoutSP {
    pub timeout: f64,
}
