#![feature(custom_attribute)]

#[macro_use] extern crate log;
#[macro_use] extern crate serde_derive;
extern crate base64;
extern crate bincode;
extern crate chrono;
extern crate clap;
extern crate env_logger;
extern crate im;
extern crate loggerv;
extern crate rand;
extern crate serde;
extern crate serde_json;

use clap::{App, Arg};
use im::list::List;
use im::list::cons;
use rand::Rng;
use std::cmp;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::convert::From;
use std::fmt;
use std::fs;
use std::io::BufReader;
use std::io::prelude::*;
use std::net::TcpStream;
use std::path::PathBuf;

use std::rc::Rc;
use std::time::Instant;

// 4.2 Online mode
// 0. Handshake

#[derive(Debug, Serialize, Deserialize)]
struct HandshakePS {
    me: Name,
}

#[derive(Debug, Serialize, Deserialize)]
struct HandshakeSP {
    you: Name,
}

type Name = String;

// 1. Setup

#[derive(Debug, Deserialize)]
struct SetupSP {
    punter: PunterId,
    punters: usize,
    map: Map,
    settings: Option<Settings>,
}

// Extnsions
#[derive(Debug, Clone, Deserialize)]
struct Settings {
    futures: Option<bool>,
    splurge: Option<bool>,
    options: Option<bool>,
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
struct OnlineSetupPS {
    ready: PunterId,
    futures: Option<Vec<Future>>,
}

#[derive(Debug, Serialize)]
struct Future {
    source: SiteId,
    target: SiteId,
}

type PunterId = usize;
type Nat = u64;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Map {
    sites: Vec<Site>,
    rivers: Vec<River>,
    mines: Vec<SiteId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Site {
    id: SiteId,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct River {
    source: SiteId,
    target: SiteId,
}

type SiteId = Nat;

// 2. Gameplay

#[derive(Debug, Serialize, Deserialize)]
struct OnlineGameplaySP {
    #[serde(rename = "move")]
    move_: Moves,
}

#[allow(dead_code)]
type OnlineGameplayPS = Move;

#[derive(Debug, Serialize, Deserialize)]
struct Moves {
    moves: Vec<Move>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Move {
    #[serde(rename = "claim")]
    ClaimBySiteId {
        punter: PunterId,
        source: SiteId,
        target: SiteId,
    },
    #[serde(rename = "pass")]
    Pass { punter: PunterId },
    #[serde(rename = "splurge")]
    Splurge {
        punter: PunterId,
        route: Vec<SiteId>,
    },
    #[serde(rename = "option")]
    Options {
        punter: PunterId,
        source: SiteId,
        target: SiteId,
    },
}

impl Move {
    fn from(claim: ClaimBySiteId) -> Self {
        Move::ClaimBySiteId {
            punter: claim.punter,
            source: claim.source,
            target: claim.target,
        }
    }
}

// 3. Scoring
#[derive(Debug, Serialize, Deserialize)]
struct OnlineScoringSP {
    stop: Scores,
}

#[derive(Debug, Serialize, Deserialize)]
struct Scores {
    moves: Vec<Move>,
    scores: Vec<Score>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Score {
    punter: PunterId,
    score: i64,
}

// 4.3 Offline mode

// 1. Setup

#[derive(Debug, Serialize)]
struct OfflineSetupPS {
    ready: PunterId,
    futures: Option<Vec<Future>>,
    state: EncodedGameState,
}

// 2. Gameplay
#[derive(Debug, Serialize, Deserialize)]
struct OfflineGamePlaySP {
    #[serde(rename = "move")]
    moves: Moves,
    state: EncodedGameState,
}

type EncodedGameState = String;

// Never uses Pass as a response
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OfflineGamePlayPS {
    claim: ClaimBySiteId,
    state: EncodedGameState,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct ClaimBySiteId {
    punter: PunterId,
    source: SiteId,
    target: SiteId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Claim {
    punter: PunterId,
    source: Node,
    target: Node,
}

impl Claim {
    fn new(punter: PunterId, source: Node, target: Node) -> Claim {
        Claim {
            punter,
            source: cmp::min(source, target),
            target: cmp::max(source, target),
        }
    }
}

// 3. Scoring
#[derive(Debug, Serialize, Deserialize)]
struct OfflineScoringSP {
    stop: Scores,
    state: EncodedGameState,
}

#[derive(Debug, Serialize, Deserialize)]
struct GameState {
    me: PunterId,
    punters: usize,
    // site_id_to_node: HashMap<SiteId, Node>,
    site_ids: Vec<SiteId>,
    mines: Vec<Node>,
    edges: Vec<Edge>,
    extension: GameExtension,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GameExtension {
    is_futures_on: bool,
    is_splurge_on: bool,
    is_options_on: bool,
    futures: Vec<Node>, // (source, target): mines.zip(futures)
    prior_passes: usize,
    prior_options: usize,
}

impl GameState {
    fn encode(&self) -> String {
        let binary: Vec<u8> = bincode::serialize(self, bincode::Infinite).unwrap();
        base64::encode(&binary)
    }

    fn decode(s: &str) -> Self {
        let binary = base64::decode(s).unwrap();
        bincode::deserialize(&binary).unwrap()
    }
}

type Node = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Edge {
    source: Node,
    target: Node,
    claimed: Claimed,
}

impl Edge {
    fn is_empty(&self) -> bool {
        self.claimed.is_empty()
    }

    fn claim(&mut self, me: PunterId, is_option: bool) {
        match self.claimed.claim(me, is_option) {
            Ok(_) => {}
            Err(_) => {
                warn!("double claiming for {:?} by {}", self, me);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Claimed {
    NotYet,
    Claimed(PunterId),
    Optioned(PunterId, PunterId),
}

impl Claimed {
    fn is_empty(&self) -> bool {
        match *self {
            Claimed::NotYet => true,
            Claimed::Claimed(_) => false,
            Claimed::Optioned(_, _) => false,
        }
    }
    fn claim(&mut self, me: PunterId, is_option: bool) -> Result<(), ()> {
        match *self {
            Claimed::NotYet => {
                *self = Claimed::Claimed(me);
                Ok(())
            }
            Claimed::Claimed(p) => {
                if is_option {
                    if p == me {
                        warn!("option for owned edge");
                        Err(())
                    } else {
                        *self = Claimed::Optioned(p, me);
                        Ok(())
                    }
                } else {
                    Err(())
                }
            }
            Claimed::Optioned(_, _) => Err(()),
        }
    }
}

// 4.4 Timeouts
#[derive(Debug, Serialize, Deserialize)]
struct TimeoutSP {
    timeout: GameState,
}

#[test]
fn parse_sample_map() {
    read_map("sample.json");
}

impl From<SetupSP> for GameState {
    fn from(setup: SetupSP) -> Self {
        // Normalize SiteIds to SideIndex
        let mut site_ids: Vec<SiteId> = setup.map.sites.iter().map(|site| site.id).collect();
        site_ids.sort();

        let site_id_to_node = make_site_id_to_node_map(&site_ids);
        assert_eq!(site_ids.len(), site_id_to_node.len());

        // Normalize mines
        let mines = setup
            .map
            .mines
            .iter()
            .map(|mine| site_id_to_node[mine])
            .collect();

        // Normalize edges
        let edges = setup
            .map
            .rivers
            .iter()
            .map(|river| {
                let s = site_id_to_node[&river.source];
                let t = site_id_to_node[&river.target];
                Edge {
                    source: cmp::min(s, t),
                    target: cmp::max(s, t),
                    claimed: Claimed::NotYet,
                }
            })
            .collect();

        GameState {
            me: setup.punter,
            punters: setup.punters,
            site_ids: site_ids,
            mines: mines,
            edges: edges,
            extension: GameExtension {
                is_futures_on: setup.settings.as_ref().and_then(|s| s.futures).unwrap_or(
                    false,
                ),
                is_splurge_on: setup.settings.as_ref().and_then(|s| s.splurge).unwrap_or(
                    false,
                ),
                is_options_on: setup.settings.as_ref().and_then(|s| s.splurge).unwrap_or(
                    false,
                ),
                futures: Vec::new(),
                prior_passes: 0,
                prior_options: 0,
            },
        }
    }
}

impl From<GameState> for Game {
    fn from(state: GameState) -> Self {
        let site_id_to_node = state.make_site_id_to_node_map();
        let edge_st_to_edge_index = state.make_edge_st_to_edge_index_map();
        let adj_edges = state.make_adj_rivers();

        Game {
            me: state.me,
            punters: state.punters,
            site_ids: state.site_ids,
            site_id_to_node: site_id_to_node,
            edge_st_to_edge_index: edge_st_to_edge_index,
            mines: state.mines,
            edges: state.edges,
            adj_edges: adj_edges,
            extension: state.extension,
        }
    }
}

fn make_site_id_to_node_map(site_ids: &Vec<SiteId>) -> HashMap<SiteId, Node> {
    let mut site_id_to_node: HashMap<SiteId, Node> = HashMap::new();
    for (index, site_id) in site_ids.iter().enumerate() {
        site_id_to_node.insert(*site_id, index);
    }
    site_id_to_node
}

impl GameState {
    fn make_site_id_to_node_map(&self) -> HashMap<SiteId, Node> {
        make_site_id_to_node_map(&self.site_ids)
    }

    fn make_edge_st_to_edge_index_map(&self) -> HashMap<(Node, Node), usize> {
        let mut st_to_index = HashMap::new();
        for (index, edge) in self.edges.iter().enumerate() {
            st_to_index.insert((edge.source, edge.target), index);
        }
        st_to_index
    }

    fn make_adj_rivers(&self) -> Vec<Vec<AdjEdge>> {
        let mut adj_edges = vec![vec![]; self.site_ids.len()];

        for e in &self.edges {
            adj_edges[e.source].push(AdjEdge {
                target: e.target,
                claimed: e.claimed.clone(),
            });
            adj_edges[e.target].push(AdjEdge {
                target: e.source,
                claimed: e.claimed.clone(),
            })
        }
        adj_edges
    }
}


fn read_n<R>(read: R, bytes_to_read: u64) -> Vec<u8>
where
    R: Read,
{
    let mut buf = vec![];
    let mut chunk = read.take(bytes_to_read);
    let status = chunk.read_to_end(&mut buf);
    match status {
        Ok(n) => assert_eq!(bytes_to_read as usize, n),
        _ => panic!("Didn't read enough"),
    }
    buf
}

fn read_server_message<R>(r: &mut R) -> String
where
    R: BufRead,
{
    let mut buf = vec![];
    let size = r.read_until(':' as u8, &mut buf).unwrap();
    debug!("{} bytes read", size);
    assert!(size > 0);

    // Drop ":".
    let n_str = std::str::from_utf8(&buf[0..buf.len() - 1]).unwrap();
    debug!("n_str: {}", n_str);
    let n: u64 = n_str.parse().unwrap();
    debug!("parsed n: {}", n);
    let buf = read_n(r, n);
    assert_eq!(buf.len(), n as usize);
    let s = String::from_utf8(buf).unwrap();
    info!("P <= S: {}", s);
    s
}

fn write_json_message<W>(w: &mut W, json: &str)
where
    W: Write,
{
    info!("P => S: sending: {}", json);
    let message = format!("{}:{}", json.as_bytes().len(), json);
    w.write(message.as_bytes()).unwrap();
    w.flush().unwrap();
}

struct OfflineIO {
    read: BufReader<std::io::Stdin>,
    stdout: std::io::Stdout,
}

impl OfflineIO {
    fn read_server_message(&mut self) -> String {
        read_server_message(&mut self.read)
    }

    fn write_json_message(&mut self, json: &str) {
        write_json_message(&mut self.stdout, json);
    }
}

struct OnlineIO {
    stream: BufReader<TcpStream>,
}

impl OnlineIO {
    fn new(server_address: &str) -> OnlineIO {
        debug!("server_address: {}", server_address);
        let stream = TcpStream::connect(server_address).unwrap();
        debug!("connected");
        OnlineIO { stream: BufReader::new(stream) }
    }

    fn read_server_message(&mut self) -> String {
        read_server_message(&mut self.stream)
    }

    fn write_json_message(&mut self, json: &str) {
        write_json_message(self.stream.get_mut(), json);
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdjEdge {
    target: Node,
    claimed: Claimed,
}

impl AdjEdge {
    fn claim(&mut self, me: PunterId, is_option: bool) {
        match self.claimed.claim(me, is_option) {
            Ok(_) => {}
            Err(_) => {
                warn!("double claiming for {:?} by {}", self, me);
            }
        }
    }
}

impl From<OfflineGamePlaySP> for Game {
    fn from(play: OfflineGamePlaySP) -> Self {
        let OfflineGamePlaySP { moves, state } = play;
        let mut game: Game = GameState::decode(&state).into();
        game.apply_moves_excluding_me(&moves.moves);
        game
    }
}

#[test]
fn read_test() {
    let input_data = b"4:abcdefg";
    let mut reader = BufReader::new(&input_data[..]);
    let s = read_server_message(&mut reader);
    assert_eq!(s, "abcd");
}

fn offline_run() {
    let mut io = OfflineIO {
        read: BufReader::new(std::io::stdin()),
        stdout: std::io::stdout(),
    };

    // 1. handshake
    let me = HandshakePS { me: "hayatox".to_string() };
    debug!("HandshakePS: {:?}", me);
    io.write_json_message(&serde_json::to_string(&me).unwrap());

    let s = io.read_server_message();
    let you: HandshakeSP = serde_json::from_str(&s).unwrap();
    debug!("HandshakeSP: {:?}", you);

    // 2. dispatch by message type
    let s = io.read_server_message();
    let data: serde_json::Value = serde_json::from_str(&s).unwrap();
    assert!(data.is_object());
    let o = data.as_object().unwrap();

    if let Some(_) = o.get("punter") {
        // setup
        let setup: SetupSP = serde_json::from_str(&s).unwrap();
        debug!("Setup: {:?}", setup);
        let state: GameState = setup.into();

        let setup = OfflineSetupPS {
            ready: state.me,
            // TODO: Support futures
            futures: None,
            state: state.encode(),
        };
        debug!("OfflineSetupPS: {:?}", setup);
        io.write_json_message(&serde_json::to_string(&setup).unwrap());
    } else if let Some(_) = o.get("move") {
        // game play
        let game_play: OfflineGamePlaySP = serde_json::from_str(&s).unwrap();
        debug!("OfflineGamePlaySP: {:?}", game_play);
        let mut game: Game = game_play.into();
        let claim_by_index = game.play();
        game.apply_claim(claim_by_index.clone(), false);
        let claim = game.convert_to_claim_by_site_id(claim_by_index);
        let state: GameState = game.into();
        let game_play = OfflineGamePlayPS {
            claim,
            state: state.encode(),
        };
        debug!("OfflineGamePlayPS: {:?}", game_play);
        io.write_json_message(&serde_json::to_string(&game_play).unwrap());
    } else if let Some(_) = o.get("stop") {
        offline_run_scoring(&s);
    } else {
        unreachable!();
    }
}

fn offline_run_scoring(s: &str) {
    let scoring: OfflineScoringSP = serde_json::from_str(s).unwrap();
    debug!("OfflineScoringSP: {:?}", scoring);
    info!("Scores: {:?}", scoring.stop.scores);

    // Re-use OfflineGamePlaySP to get the final game state.
    let game_play = OfflineGamePlaySP {
        moves: Moves { moves: scoring.stop.moves },
        state: scoring.state.into(),
    };
    let game: Game = game_play.into();
    debug!("final game state: {:?}", game);
}

fn online_run(address: &str) -> Game {
    let mut io = OnlineIO::new(address);

    // 1. handshake
    let me = HandshakePS { me: "hayatox".to_string() };
    debug!("HandshakePS: {:?}", me);
    io.write_json_message(&serde_json::to_string(&me).unwrap());

    let s = io.read_server_message();
    let you: HandshakeSP = serde_json::from_str(&s).unwrap();
    debug!("HandshakeSP: {:?}", you);

    // 2. setup
    let s = io.read_server_message();
    let setup: SetupSP = serde_json::from_str(&s).unwrap();
    debug!("SetupSP: {:?}", setup);

    let state: GameState = setup.into();
    let mut game: Game = state.into();

    // Support futures
    // game.setup_futures();

    let setup_ps = OnlineSetupPS {
        ready: game.me,
        futures: game.convert_setup_futures_message(),
    };
    io.write_json_message(&serde_json::to_string(&setup_ps).unwrap());

    debug!("Game: {:?}", game);

    // 3. loop
    while let Some(claim) = game.run_online_turn(&io.read_server_message()) {
        // TODO: Support option
        game.apply_claim(claim.clone(), false);
        let claim = game.convert_to_claim_by_site_id(claim);
        let mov = Move::from(claim);
        io.write_json_message(&serde_json::to_string(&mov).unwrap());
    }
    game
}

#[derive(Debug, Clone, PartialEq)]
struct EdgeCandidate {
    dist: u32,
    source: Node,
    target: Node,
}

#[derive(Debug, Default)]
struct Game {
    me: PunterId,
    punters: usize,
    site_ids: Vec<SiteId>,
    site_id_to_node: HashMap<SiteId, Node>,
    mines: Vec<Node>,
    edge_st_to_edge_index: HashMap<(Node, Node), usize>,
    edges: Vec<Edge>,
    // edges: (3, 5), (3, 7)
    // -> adj_river[3] = vec![AdjEdge { target: 5, claimed},  AdjEdge { targer: 7, claimed} ]
    adj_edges: Vec<Vec<AdjEdge>>,
    extension: GameExtension,
}

type EdgeWeights = Vec<u64>;

impl Game {
    fn setup_futures(&mut self) {
        if !self.extension.is_futures_on {
            warn!("setup_futures() is called for a game where futures is disabled");
            return;
        }
        // random pick.
        self.extension.futures = (0..self.site_ids.len())
            .filter(|node| self.mines.binary_search(node).is_err())
            .take(self.mines.len())
            .collect();
    }

    fn convert_setup_futures_message(&self) -> Option<Vec<Future>> {
        if self.extension.is_futures_on {
            Some(
                self.mines
                    .iter()
                    .zip(self.extension.futures.iter())
                    .map(|(mine, future)| {
                        Future {
                            source: self.site_ids[*mine],
                            target: self.site_ids[*future],
                        }
                    })
                    .collect(),
            )
        } else {
            None
        }
    }


    fn convert_to_claim_by_site_id(&self, c: Claim) -> ClaimBySiteId {
        ClaimBySiteId {
            punter: c.punter,
            source: self.node_to_site_id(c.source),
            target: self.node_to_site_id(c.target),
        }
    }

    fn convert_to_claim(&self, claim: ClaimBySiteId) -> Claim {
        let s = self.site_id_to_node[&claim.source];
        let t = self.site_id_to_node[&claim.target];
        Claim::new(claim.punter, s, t)
    }

    fn node_to_site_id(&self, i: Node) -> SiteId {
        self.site_ids[i]
    }

    fn apply_claim_by_site_id(&mut self, claim: ClaimBySiteId) {
        debug!("apply_claim_by_site_id: {:?}", claim);
        let c = self.convert_to_claim(claim);
        self.apply_claim(c, false);
    }

    fn apply_claim(&mut self, claim: Claim, is_option: bool) {
        debug!("claim: {:?}", claim);

        assert!(claim.source < claim.target);

        // 1. Update self.edges
        // let river: &mut Edge = &mut self.edges[self.edge_st_to_edge_index[&(s, t)]];
        match self.edge_st_to_edge_index.get(
            &(claim.source, claim.target),
        ) {
            Some(index) => {
                let river: &mut Edge = &mut self.edges[*index];
                assert_eq!(claim.source, river.source);
                assert_eq!(claim.target, river.target);
                river.claim(claim.punter, is_option);

                // 2. Update self.adj_edges
                // TODO: This takes O(N^2) in worst.
                {
                    let river: &mut AdjEdge = self.adj_edges[claim.source]
                        .iter_mut()
                        .find(|adj_river| adj_river.target == claim.target)
                        .unwrap();
                    river.claim(claim.punter, is_option);
                }
                {
                    let river: &mut AdjEdge = self.adj_edges[claim.target]
                        .iter_mut()
                        .find(|adj_river| adj_river.target == claim.source)
                        .unwrap();
                    river.claim(claim.punter, is_option);
                }

            }
            None => {
                warn!("invalid claim: {:?}", claim);
                warn!("edge_st_to_edge_index: {:?}", self.edge_st_to_edge_index);
            }
        }
    }

    fn apply_option(&mut self, claim: ClaimBySiteId) {
        debug!("apply_option: {:?}", claim);
        let c = self.convert_to_claim(claim);
        self.apply_claim(c, true);
    }

    fn play(&self) -> Claim {
        let now = Instant::now();
        // self.play_stupid()
        // self.play_greedy()
        let claim = self.play_edge_weight();
        let secs = now.elapsed().as_secs();
        if secs >= 1 {
            warn!("{} secs passed", secs);
        }
        claim
    }

    fn play_stupid(&self) -> Claim {
        let empty_edge = self.edges.iter().find(|river| river.is_empty()).unwrap();
        Claim::new(self.me, empty_edge.source, empty_edge.target)
    }

    fn play_greedy(&self) -> Claim {
        let mut res = Vec::new();
        for mine in &self.mines {
            res.append(&mut self.collect_next_unvisited(*mine));
        }
        res.sort_by_key(|c| -(c.dist as i32));
        debug!("res: {:?}", res);
        if res.is_empty() {
            self.play_stupid()
        } else {
            Claim::new(self.me, res[0].source, res[0].target)
        }
    }

    fn play_edge_weight(&self) -> Claim {
        let now = Instant::now();
        let claim = self.find_valuable_edge_by_weight().unwrap_or(
            self.play_stupid(),
        );

        let elapsed = now.elapsed();
        let secs = elapsed.as_secs();
        let nano = elapsed.subsec_nanos();
        debug!(
            "{} secs passed",
            secs as f64 + (nano as f64) / 1_000_000_000.0
        );
        if secs >= 1 {
            warn!(
                "{} secs passed",
                secs as f64 + (nano as f64) / 1_000_000_000.0
            );
        }
        claim
    }

    fn calc_shortest_dist_from(&self, mine: Node) -> Vec<usize> {
        #[derive(Debug, Clone, PartialEq)]
        struct Entry {
            index: Node,
            dist: usize,
        }

        let mut dist = vec![0; self.site_ids.len()];
        {
            let mut q = VecDeque::new();
            q.push_back(Entry {
                index: mine,
                dist: 0,
            });
            let mut visited = HashSet::new();
            visited.insert(mine);


            while let Some(s) = q.pop_front() {
                dist[s.index] = s.dist;
                for adj in self.adj_edges[s.index].iter() {
                    if visited.contains(&adj.target) {
                        continue;
                    }
                    visited.insert(adj.target);
                    q.push_back(Entry {
                        index: adj.target,
                        dist: s.dist + 1,
                    });
                }
            }
        }
        dist
    }


    fn find_valuable_edge_by_weight(&self) -> Option<Claim> {
        let mut edge_weights: EdgeWeights = vec![0; self.edges.len()];
        for mine in &self.mines {
            self.calc_edge_weight_for(&mut edge_weights, *mine);
        }
        let mut v: Vec<(usize, u64)> = edge_weights.into_iter().enumerate().collect();
        v.sort_by_key(|&(_, weight)| -(weight as i64));
        v.iter()
            .find(|&&(index, _)| self.edges[index].is_empty())
            .map(|&(index, _)| {
                let e = &self.edges[index];
                Claim::new(self.me, e.source, e.target)
            })
    }

    fn edge_index(&self, s: usize, t: usize) -> usize {
        self.edge_st_to_edge_index[&(cmp::min(s, t), cmp::max(s, t))]
    }

    fn calc_edge_weight_for(&self, edge_weights: &mut EdgeWeights, mine: Node) {
        // TODO: Encode this to GameState
        let dist = self.calc_shortest_dist_from(mine);

        type EdgeIndex = usize;

        #[derive(Debug, Clone, PartialEq)]
        struct Entry {
            source: Node,
            // edge_backtrack: Vec<EdgeIndex>,
            edge_backtrack: Rc<List<EdgeIndex>>,
        }

        let mut q = VecDeque::new();
        q.push_back(Entry {
            source: mine,
            edge_backtrack: Rc::new(List::new()),
        });
        let mut visited = HashSet::new();
        visited.insert(mine);

        while let Some(entry) = q.pop_front() {
            let Entry {
                source,
                edge_backtrack,
            } = entry;
            for ei in edge_backtrack.iter() {
                edge_weights[*ei] += (dist[source] * dist[source]) as u64;
            }

            for adj in self.adj_edges[source].iter() {
                if visited.contains(&adj.target) {
                    continue;
                }
                match adj.claimed {
                    Claimed::NotYet => {
                        visited.insert(adj.target);
                        let edge_index = self.edge_index(source, adj.target);
                        let nb = Rc::new(cons(edge_index, edge_backtrack.clone()));
                        q.push_back(Entry {
                            source: adj.target,
                            edge_backtrack: nb,
                        });
                    }
                    Claimed::Claimed(p) => {
                        if p == self.me {
                            visited.insert(adj.target);
                            let edge_index = self.edge_index(source, adj.target);
                            let nb = Rc::new(cons(edge_index, edge_backtrack.clone()));
                            q.push_back(Entry {
                                source: adj.target,
                                edge_backtrack: nb,
                            });
                        }
                    }
                    Claimed::Optioned(p0, p1) => {
                        if p0 == self.me || p1 == self.me {
                            visited.insert(adj.target);
                            let edge_index = self.edge_index(source, adj.target);
                            let nb = Rc::new(cons(edge_index, edge_backtrack.clone()));
                            q.push_back(Entry {
                                source: adj.target,
                                edge_backtrack: nb,
                            });
                        }
                    }
                }
            }
        }
    }

    fn collect_next_unvisited(&self, mine: Node) -> Vec<EdgeCandidate> {
        #[derive(Debug, Clone, PartialEq)]
        struct Entry {
            index: Node,
            dist: u32,
        }

        let mut res = Vec::new();
        let mut q = VecDeque::new();
        q.push_back(Entry {
            index: mine,
            dist: 0,
        });
        let mut visited = HashSet::new();
        visited.insert(mine);

        while let Some(s) = q.pop_front() {
            for adj in self.adj_edges[s.index].iter() {
                debug!("adj: {:?}", adj);
                if visited.contains(&adj.target) {
                    continue;
                }
                match adj.claimed {
                    Claimed::NotYet => {
                        res.push(EdgeCandidate {
                            dist: s.dist + 1,
                            source: s.index,
                            target: adj.target,
                        });
                    }
                    Claimed::Claimed(p) => {
                        if p == self.me {
                            visited.insert(adj.target);
                            q.push_back(Entry {
                                index: adj.target,
                                dist: s.dist + 1,
                            });

                        }
                    }
                    Claimed::Optioned(p0, p1) => {
                        if p0 == self.me || p1 == self.me {
                            visited.insert(adj.target);
                            q.push_back(Entry {
                                index: adj.target,
                                dist: s.dist + 1,
                            });

                        }
                    }
                }
            }
        }
        res
    }

    fn run_online_turn(&mut self, server_message: &str) -> Option<Claim> {
        let data: serde_json::Value = serde_json::from_str(server_message).unwrap();
        assert!(data.is_object());
        let o = data.as_object().unwrap();
        if let Some(_) = o.get("move") {
            Some(self.run_online_move(
                serde_json::from_str(&server_message).unwrap(),
            ))
        } else if let Some(_) = o.get("stop") {
            self.run_online_stop(serde_json::from_str(&server_message).unwrap());
            None
        } else {
            unreachable!();
        }
    }

    fn apply_moves_excluding_me(&mut self, moves: &Vec<Move>) {
        debug!(
            "apply_moves_excluding_me: me: {}, moves: {:?}",
            self.me,
            moves
        );
        for m in moves {
            match *m {
                Move::ClaimBySiteId {
                    punter,
                    source,
                    target,
                } => {
                    if self.me != punter {
                        self.apply_claim_by_site_id(ClaimBySiteId {
                            punter,
                            source,
                            target,
                        })
                    }
                }
                Move::Splurge { punter, ref route } => {
                    if self.me != punter {
                        for river in route.windows(2) {
                            self.apply_claim_by_site_id(ClaimBySiteId {
                                punter,
                                source: river[0],
                                target: river[1],
                            })
                        }
                    }
                }
                Move::Options {
                    punter,
                    source,
                    target,
                } => {
                    if self.me != punter {
                        self.apply_option(ClaimBySiteId {
                            punter,
                            source,
                            target,
                        });
                    }
                }
                Move::Pass { .. } => {}
            }
        }
    }

    fn score(&self, p: PunterId) -> i64 {
        self.mines
            .iter()
            .enumerate()
            .map(|(i, mine)| {
                self.score_for_mine(*mine, self.extension.futures.get(i).cloned(), p)
            })
            .sum()
    }

    fn score_for_mine(&self, mine: Node, future: Option<Node>, p: PunterId) -> i64 {

        let dist = self.calc_shortest_dist_from(mine);

        // Reachable nodes
        let mut q = VecDeque::new();
        q.push_back(mine);
        let mut visited = HashSet::new();
        visited.insert(mine);

        while let Some(s) = q.pop_front() {
            for adj in self.adj_edges[s].iter() {
                if visited.contains(&adj.target) {
                    continue;
                }
                match adj.claimed {
                    Claimed::NotYet => {}
                    Claimed::Claimed(p0) => {
                        if p0 == p {
                            visited.insert(adj.target);
                            q.push_back(adj.target);
                        }
                    }
                    Claimed::Optioned(p0, p1) => {
                        if p0 == p || p1 == p {
                            visited.insert(adj.target);
                            q.push_back(adj.target);
                        }
                    }
                }
            }
        }
        let mut score: i64 = 0;
        for s in &visited {
            let d = dist[*s] as i64;
            score += d * d;
        }
        if p == self.me {
            match future {
                Some(target) => {
                    let d = dist[target] as i64;
                    if visited.contains(&target) {
                        score += d * d * d;
                    } else {
                        score -= d * d * d;
                    }
                }
                None => {}
            }
        }
        score
    }

    fn run_online_move(&mut self, game_play: OnlineGameplaySP) -> Claim {
        self.apply_moves_excluding_me(&game_play.move_.moves);
        self.play()
    }

    fn run_online_stop(&mut self, scoring: OnlineScoringSP) {
        debug!("Scoring: {:?}", scoring);
        info!("Scores: {:?}", scoring.stop.scores);
        self.apply_moves_excluding_me(&scoring.stop.moves);
        debug!("final game state: {:?}", self);
    }

    fn print_punter_summary(&self, p: PunterId) {
        let owned_rivers: Vec<(Node, Node)> = self.edges
            .iter()
            .flat_map(|ref r| match r.claimed {
                Claimed::NotYet => None,
                Claimed::Claimed(p0) => {
                    if p0 == p {
                        Some((r.source, r.target))
                    } else {
                        None
                    }
                }
                Claimed::Optioned(p0, p1) => {
                    if p0 == p || p1 == p {
                        Some((r.source, r.target))
                    } else {
                        None
                    }
                }

            })
            .collect();
        info!(
            "Punter {}: Score: {}, Ownes {} edges: {:?}",
            p,
            self.score(p),
            owned_rivers.len(),
            owned_rivers
        );
    }

    fn print_summary(&self) {
        info!("Me: {}", self.me);
        for p in 0..self.punters {
            self.print_punter_summary(p as PunterId);
        }
    }
}

fn read_map(map_name: &str) -> Map {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("task/maps");
    path.push(map_name);
    let mut f = fs::File::open(&path).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn battle_run(write_graph: bool) {
    type BotName = String;

    #[derive(Debug)]
    struct BotStat {
        battles: u64,
        total_score: i64,
    }

    impl fmt::Display for BotStat {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(
                f,
                "avg: {} (= {} / {})",
                if self.battles == 0 {
                    0
                } else {
                    self.total_score / (self.battles as i64)
                },
                self.total_score,
                self.battles
            )
        }
    }

    for map_name in &[
        // "sample.json",
        "lambda.json",
        "Sierpinski-triangle.json",
        "circle.json",
        "randomMedium.json",
        "randomSparse.json",
        "tube.json",
        "oxford-center-sparse.json",
        "oxford.json",
        "edinburgh-sparse.json",
        "boston-sparse.json",
        "nara-sparse.json",
        "van-city-sparse.json",
        "gothenburg-sparse.json",
    ]
    {
        let mut stats: BTreeMap<BotName, BotStat> = BTreeMap::new();
        for i in 0..10 {
            let map = read_map(map_name);
            let punters = 4;
            let mut settings: Settings = Default::default();
            let mut battle = Battle::new(
                map,
                punters,
                if i % 2 == 0 {
                    settings
                } else {
                    settings.futures = Some(true);
                    settings
                },
            );
            let scores = battle.run();
            let bot_names = scores
                .iter()
                .map(|score| battle.bots[score.punter].name())
                .collect::<Vec<_>>();
            info!("bots: {:?}", bot_names);
            info!("scores: {:?}", scores);
            for score in scores {
                let bot_name = battle.bots[score.punter].name();
                let e = stats.entry(bot_name).or_insert(BotStat {
                    battles: 0,
                    total_score: 0,
                });
                e.battles += 1;
                e.total_score += score.score;
            }
            if write_graph {
                battle.write_vis_graph_json();
            }
        }
        println!("stats: map: {}", map_name);
        for (bot_name, bot_stat) in stats {
            println!("bot: {}, stats: {}", bot_name, bot_stat);
        }
        println!("");
    }
}

struct Battle {
    map: Map,
    punters: usize,
    bots: Vec<Box<Bot>>,
    settings: Settings,
    moves: Vec<Move>,
}

impl Battle {
    fn new(map: Map, punters: usize, settings: Settings) -> Battle {
        Battle {
            map,
            punters,
            bots: (0..punters).map(|_| random_bot()).collect(),
            settings,
            moves: Vec::new(),
        }
    }

    fn run(&mut self) -> Vec<Score> {
        debug!("battle start");
        assert_eq!(self.bots.len(), self.punters);
        for (punter_id, bot) in self.bots.iter_mut().enumerate() {
            bot.setup(SetupSP {
                punter: punter_id,
                punters: self.punters,
                map: self.map.clone(),
                settings: Some(self.settings.clone()),
            });
        }

        let mut last_moves: Vec<Move> = (0..self.punters)
            .map(|p| Move::Pass { punter: p as usize })
            .collect();
        let turns = self.map.rivers.len();
        for p in (0..self.punters).cycle().take(turns) {
            debug!("battle gameplay");
            let mov = self.bots[p].gameplay(&last_moves);
            self.moves.push(mov.clone());
            last_moves[p] = mov;
        }
        debug!("battle end");
        (0..self.punters)
            .map(|i| {
                let bot_index = (i + turns) % self.punters;
                let score = self.bots[bot_index].stop(&last_moves);
                // Avoid duble claiming
                last_moves[bot_index] = Move::Pass { punter: bot_index };
                score
            })
            .collect()
    }

    fn write_vis_graph_json(&self) {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let local = chrono::Local::now();
        path.push("visualizer/data");
        path.push(local.format("%Y-%m-%d-%H-%M-%S-%f.json").to_string());

        let vis = VisGraph {
            map: self.map.clone(),
            moves: self.moves.clone(),
        };

        let json = serde_json::to_string(&vis).unwrap();
        let mut n = std::fs::File::create(&path).unwrap();
        info!(">> Writing graph as {}", path.display());
        n.write(json.as_bytes()).unwrap();

        let mut link = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        link.push("visualizer/latest.json");

        if link.exists() {
            fs::remove_file(&link).unwrap();
        }
        std::os::unix::fs::symlink(&path, &link).unwrap();
    }
}

fn random_bot() -> Box<Bot> {
    let mut rng = rand::thread_rng();
    match rng.gen_range(0, 2) {
        0 => Box::new(GreedyBot::new(false)),
        1 => Box::new(EdgeWeightBot::new()),
        // 2 => Box::new(StupidBot::new()),
        _ => unreachable!(),
    }
}

trait Bot {
    fn name(&self) -> String;
    fn take_game(&mut self, game: Game);
    fn game_mut(&mut self) -> &mut Game;
    fn play(&self) -> Claim;

    fn setup(&mut self, setup: SetupSP) {
        let state: GameState = setup.into();
        self.take_game(state.into());
    }
    fn gameplay(&mut self, moves: &Vec<Move>) -> Move {
        {
            let game: &mut Game = self.game_mut();
            game.apply_moves_excluding_me(moves);
        }
        let claim = self.play();
        let game: &mut Game = self.game_mut();
        game.apply_claim(claim.clone(), false);
        Move::from(game.convert_to_claim_by_site_id(claim))
    }
    fn stop(&mut self, moves: &Vec<Move>) -> Score {
        let game: &mut Game = self.game_mut();
        game.apply_moves_excluding_me(moves);
        Score {
            punter: game.me,
            score: game.score(game.me),
        }
    }
}

#[derive(Debug)]
struct GreedyBot {
    game: Option<Game>,
    use_futures: bool,
}

impl GreedyBot {
    fn new(use_futures: bool) -> Self {
        Self {
            game: std::default::Default::default(),
            use_futures,
        }
    }
}

impl Bot for GreedyBot {
    fn name(&self) -> String {
        if self.use_futures {
            "GreedyBot (use futures)".to_string()
        } else {
            "GreedyBot".to_string()
        }
    }
    fn take_game(&mut self, game: Game) {
        self.game = Some(game);
        if self.game_mut().extension.is_futures_on && self.use_futures {
            self.game_mut().setup_futures();
        }
    }
    fn game_mut(&mut self) -> &mut Game {
        self.game.as_mut().unwrap()
    }
    fn play(&self) -> Claim {
        self.game.as_ref().unwrap().play_greedy()
    }
}

#[derive(Debug)]
struct EdgeWeightBot {
    game: Option<Game>,
}

impl EdgeWeightBot {
    fn new() -> Self {
        Self { game: std::default::Default::default() }
    }
}

impl Bot for EdgeWeightBot {
    fn name(&self) -> String {
        "EdgeWeightBot".to_string()
    }
    fn take_game(&mut self, game: Game) {
        self.game = Some(game);
    }
    fn game_mut(&mut self) -> &mut Game {
        self.game.as_mut().unwrap()
    }
    fn play(&self) -> Claim {
        self.game.as_ref().unwrap().play_edge_weight()
    }
}

#[derive(Debug, Serialize)]
struct VisGraph {
    map: Map,
    moves: Vec<Move>,
}

impl From<Game> for GameState {
    fn from(game: Game) -> Self {
        GameState {
            me: game.me,
            punters: game.punters,
            site_ids: game.site_ids,
            mines: game.mines,
            edges: game.edges,
            extension: game.extension,
        }
    }
}

fn build_cli() -> App<'static, 'static> {
    App::new("icfp2017")
        .version("1.0")
        .arg(Arg::with_name("v").short("v").multiple(true).help(
            "Sets the level of verbosity",
        ))
        .arg(Arg::with_name("port").short("p").long("port").takes_value(
            true,
        ))
        .arg(Arg::with_name("graph").short("g").long("graph"))
        .arg(Arg::with_name("battle").short("b").long("battle"))
}

fn main() {
    let matches = build_cli().get_matches();
    let v = matches.occurrences_of("v");
    match std::env::var("RUST_LOG") {
        Ok(_) => {
            env_logger::init().unwrap();
            if v > 0 {
                warn!("-v flag is ignored when RUST_LOG is set")
            }
        }
        Err(_) => {
            loggerv::init_with_verbosity(v).unwrap();
        }
    }
    debug!("Hello");
    if let Some(port) = matches.value_of("port") {
        debug!(">>> online_run");
        let game = online_run(&format!("{}:{}", "punter.inf.ed.ac.uk", port));
        game.print_summary();
    } else if matches.is_present("battle") {
        debug!(">>> battle_run");
        battle_run(matches.is_present("graph"));
    } else {
        debug!(">>> offline_run");
        offline_run();
    }
    debug!("Bye");
}
