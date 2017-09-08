use base64;
use bincode;
use punter::prelude::*;
use punter::protocol::*;
use std::cell::RefCell;
use std::cmp;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

pub type Node = usize;
pub type EdgeIndex = usize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source: Node,
    pub target: Node,
    pub claimed: Claimed,
}

impl Edge {
    pub fn is_empty(&self) -> bool {
        self.claimed.is_empty()
    }

    pub fn claim(&mut self, me: PunterId, is_option: bool) {
        if self.claimed.claim(me, is_option).is_err() {
            warn!("double claiming for {:?} by {}", self, me);
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Game {
    pub me: PunterId,
    punters: usize,
    site_ids: Vec<SiteId>,
    mines: Vec<Node>,
    edges: Vec<Edge>,
    extension: GameExtension,
    // Pre-computed values
    site_id_to_node: HashMap<SiteId, Node>,
    edge_st_to_edge_index: HashMap<(Node, Node), EdgeIndex>,
    adj_edges: Vec<Vec<AdjEdge>>,
    dist_from_mine: Vec<Vec<usize>>, // dist[0][3] -> dist(mines[0], node3)
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct GameExtension {
    pub is_futures_on: bool,
    pub is_splurge_on: bool,
    pub is_options_on: bool,
    pub futures: Vec<Node>, // (source, target): mines.zip(futures)
    pub prior_passes: usize,
    pub prior_options: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeClaim {
    pub punter: PunterId,
    pub source: Node,
    pub target: Node,
}

impl EdgeClaim {
    pub fn new(punter: PunterId, source: Node, target: Node) -> EdgeClaim {
        EdgeClaim {
            punter,
            source: cmp::min(source, target),
            target: cmp::max(source, target),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Claimed {
    NotYet,
    Claimed(PunterId),
    Optioned(PunterId, PunterId),
}

impl Claimed {
    pub fn is_empty(&self) -> bool {
        match *self {
            Claimed::NotYet => true,
            Claimed::Claimed(_) => false,
            Claimed::Optioned(_, _) => false,
        }
    }
    pub fn claim(&mut self, me: PunterId, is_option: bool) -> Result<(), ()> {
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

impl From<SetupSP> for Game {
    fn from(setup: SetupSP) -> Self {
        let mut site_ids: Vec<SiteId> = setup.map.sites.iter().map(|site| site.id).collect();
        site_ids.sort();

        let site_id_to_node = {
            let mut site_id_to_node: HashMap<SiteId, Node> = HashMap::new();
            for (index, site_id) in site_ids.iter().enumerate() {
                site_id_to_node.insert(*site_id, index);
            }
            site_id_to_node
        };
        assert_eq!(site_ids.len(), site_id_to_node.len());

        let mines: Vec<Node> = setup
            .map
            .mines
            .iter()
            .map(|mine| site_id_to_node[mine])
            .collect();

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
            .collect::<Vec<Edge>>();

        let edge_st_to_edge_index = {
            let mut st_to_index = HashMap::new();
            for (index, edge) in edges.iter().enumerate() {
                st_to_index.insert((edge.source, edge.target), index);
            }
            st_to_index
        };

        let adj_edges = {
            let mut adj_edges = vec![vec![]; site_ids.len()];

            for (index, e) in edges.iter().enumerate() {
                adj_edges[e.source].push(AdjEdge {
                    target: e.target,
                    edge_index: index,
                });
                adj_edges[e.target].push(AdjEdge {
                    target: e.source,
                    edge_index: index,
                })
            }
            adj_edges
        };

        let dist_from_mine = mines
            .iter()
            .map(|mine| {
                struct Entry {
                    index: Node,
                    dist: usize,
                }

                let mut dist = vec![0; site_ids.len()];
                let mut q = VecDeque::new();
                q.push_back(Entry {
                    index: *mine,
                    dist: 0,
                });
                let mut visited = HashSet::new();
                visited.insert(*mine);

                while let Some(s) = q.pop_front() {
                    dist[s.index] = s.dist;
                    for adj in adj_edges[s.index].iter() {
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
                dist
            })
            .collect();

        Game {
            me: setup.punter,
            punters: setup.punters,
            site_ids,
            mines,
            edges,
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
            site_id_to_node,
            edge_st_to_edge_index,
            adj_edges,
            dist_from_mine,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdjEdge {
    pub target: Node,
    pub edge_index: EdgeIndex,
}

impl From<OfflineGamePlaySP> for Game {
    fn from(play: OfflineGamePlaySP) -> Self {
        let OfflineGamePlaySP { moves, state } = play;
        assert!(state.is_string());
        let mut game = Game::decode(&state.as_str().unwrap());
        game.apply_moves_excluding_me(moves.moves);
        game
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EdgeCandidate {
    dist: u32,
    source: Node,
    target: Node,
}

type EdgeWeights = Vec<u64>;

#[derive(Debug, Copy, Clone)]
pub enum Strategy {
    Stupid,
    EdgeWeight,
}

impl Game {
    pub fn encode(&self) -> String {
        let binary: Vec<u8> = bincode::serialize(self, bincode::Infinite).unwrap();
        base64::encode(&binary)
    }

    pub fn decode(s: &str) -> Self {
        let binary = base64::decode(s).unwrap();
        bincode::deserialize(&binary).unwrap()
    }

    pub fn setup_futures(&mut self) {
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

    pub fn is_futures_on(&self) -> bool {
        self.extension.is_futures_on
    }

    pub fn convert_setup_futures_message(&self) -> Option<Vec<Future>> {
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

    pub fn convert_to_claim(&self, c: EdgeClaim) -> Claim {
        Claim {
            punter: c.punter,
            source: self.node_to_site_id(c.source),
            target: self.node_to_site_id(c.target),
        }
    }

    fn convert_to_edge_claim(&self, claim: Claim) -> EdgeClaim {
        let s = self.site_id_to_node[&claim.source];
        let t = self.site_id_to_node[&claim.target];
        EdgeClaim::new(claim.punter, s, t)
    }

    fn node_to_site_id(&self, i: Node) -> SiteId {
        self.site_ids[i]
    }

    fn apply_claim(&mut self, claim: Claim) {
        let c = self.convert_to_edge_claim(claim);
        self.apply_edge_claim(c, false);
    }

    pub fn apply_edge_claim(&mut self, claim: EdgeClaim, is_option: bool) {
        debug!("edge-claim: {:?}", claim);
        assert!(claim.source < claim.target);
        match self.edge_st_to_edge_index.get(
            &(claim.source, claim.target),
        ) {
            Some(index) => {
                let edge: &mut Edge = &mut self.edges[*index];
                assert_eq!(claim.source, edge.source);
                assert_eq!(claim.target, edge.target);
                edge.claim(claim.punter, is_option);
            }
            None => {
                warn!("invalid claim: {:?}", claim);
            }
        }
    }

    fn apply_option(&mut self, claim: Claim) {
        debug!("apply_option: {:?}", claim);
        let c = self.convert_to_edge_claim(claim);
        self.apply_edge_claim(c, true);
    }

    pub fn play(&self, strategy: Strategy) -> Move {
        let now = Instant::now();
        let edge_claim = {
            match strategy {
                Strategy::Stupid => self.play_stupid(),
                Strategy::EdgeWeight => self.play_edge_weight(),
            }
        };
        let secs = now.elapsed().as_secs();
        if secs >= 1 {
            warn!("{} secs passed", secs);
        }
        Move::from(self.convert_to_claim(edge_claim))
    }

    fn play_stupid(&self) -> EdgeClaim {
        let empty_edge = self.edges.iter().find(|e| e.is_empty()).unwrap();
        EdgeClaim::new(self.me, empty_edge.source, empty_edge.target)
    }

    pub fn play_edge_weight(&self) -> EdgeClaim {
        let now = Instant::now();
        let claim = self.find_valuable_edge_by_weight();
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

    fn find_valuable_edge_by_weight(&self) -> EdgeClaim {
        let edge_weights: Rc<RefCell<EdgeWeights>> = Rc::new(RefCell::new(vec![0; self.edges.len()]));
        for (mine, dist_from_mine) in self.mines.iter().zip(self.dist_from_mine.iter()) {
            self.calc_edge_weight_for(edge_weights.clone(), *mine, dist_from_mine);
        }
        assert_eq!(Rc::strong_count(&edge_weights), 1);
        Rc::try_unwrap(edge_weights)
            .unwrap()
            .into_inner()
            .into_iter()
            .zip(self.edges.iter())
            .filter(|&(_, e)| e.is_empty())
            .max_by_key(|&(weight, _)| weight)
            .map(|(_, e)| EdgeClaim::new(self.me, e.source, e.target))
            .unwrap()
    }

    fn edge_index(&self, s: usize, t: usize) -> usize {
        self.edge_st_to_edge_index[&(cmp::min(s, t), cmp::max(s, t))]
    }

    fn calc_edge_weight_for(&self, edge_weights: Rc<RefCell<EdgeWeights>>, mine: Node, dist_from_mine: &[usize]) {
        #[derive(Debug, Clone, PartialEq)]
        struct Entry {
            source: Node,
            weight_promise: u64,
            prev: Option<(EdgeIndex, Rc<RefCell<Entry>>)>,
            edge_weights: Rc<RefCell<EdgeWeights>>,
        }

        impl Drop for Entry {
            fn drop(&mut self) {
                match self.prev {
                    Some((edge_index, ref prev_entry)) => {
                        self.edge_weights.borrow_mut()[edge_index] += self.weight_promise;
                        prev_entry.borrow_mut().weight_promise += self.weight_promise;
                    }
                    None => {}
                }
            }
        }

        let mut q = VecDeque::new();
        q.push_back(Rc::new(RefCell::new(Entry {
            source: mine,
            weight_promise: 0,
            prev: None,
            edge_weights: edge_weights,
        })));
        let mut visited = HashSet::new();
        visited.insert(mine);

        while let Some(entry) = q.pop_front() {
            let source = entry.borrow().source;
            for adj in self.adj_edges[source].iter() {
                let target = adj.target;
                if visited.contains(&target) {
                    continue;
                }

                if match self.edges[adj.edge_index].claimed {
                    Claimed::NotYet => true,
                    Claimed::Claimed(p) => p == self.me,
                    Claimed::Optioned(p0, p1) => p0 == self.me || p1 == self.me,
                }
                {
                    visited.insert(target);
                    q.push_back(Rc::new(RefCell::new(Entry {
                        source: adj.target,
                        weight_promise: (dist_from_mine[target] * dist_from_mine[target]) as u64,
                        prev: Some((self.edge_index(source, target), entry.clone())),
                        edge_weights: entry.borrow().edge_weights.clone(),
                    })));
                }
            }
        }
    }

    pub fn apply_move(&mut self, m: Move) {
        match m {
            Move::Claim { claim } => self.apply_claim(claim),
            Move::Splurge { splurge: Splurge { punter, route } } => {
                for river in route.windows(2) {
                    self.apply_claim(Claim {
                        punter,
                        source: river[0],
                        target: river[1],
                    })
                }
            }
            Move::Option_ { option } => {
                self.apply_option(option);
            }
            Move::Pass { .. } => {}
        }
    }

    pub fn apply_moves_excluding_me(&mut self, moves: Vec<Move>) {
        let me = self.me;
        for m in moves.into_iter().filter(|m| !m.claimed_by(me)) {
            self.apply_move(m);
        }
    }

    pub fn score(&self, p: PunterId) -> i64 {
        self.mines
            .iter()
            .zip(self.dist_from_mine.iter())
            .enumerate()
            .map(|(i, (mine, dist_from_mine))| {
                self.score_for_mine(
                    *mine,
                    dist_from_mine,
                    self.extension.futures.get(i).cloned(),
                    p,
                )
            })
            .sum()
    }

    fn score_for_mine(&self, mine: Node, dist_from_mine: &[usize], future: Option<Node>, p: PunterId) -> i64 {
        let mut q = VecDeque::new();
        q.push_back(mine);
        let mut visited = HashSet::new();
        visited.insert(mine);

        while let Some(s) = q.pop_front() {
            for adj in self.adj_edges[s].iter() {
                if visited.contains(&adj.target) {
                    continue;
                }
                if match self.edges[adj.edge_index].claimed {
                    Claimed::NotYet => false,
                    Claimed::Claimed(p0) => p0 == p,
                    Claimed::Optioned(p0, p1) => p0 == p || p1 == p,
                }
                {
                    visited.insert(adj.target);
                    q.push_back(adj.target);

                }
            }
        }
        let mut score: i64 = 0;
        for s in &visited {
            let d = dist_from_mine[*s] as i64;
            score += d * d;
        }
        if p == self.me {
            match future {
                Some(target) => {
                    let d = dist_from_mine[target] as i64;
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

    fn print_punter_summary(&self, p: PunterId) {
        let owned_edges: Vec<(Node, Node)> = self.edges
            .iter()
            .flat_map(|ref r| if match r.claimed {
                Claimed::NotYet => false,
                Claimed::Claimed(p0) => p0 == p,
                Claimed::Optioned(p0, p1) => p0 == p || p1 == p,
            }
            {
                Some((r.source, r.target))
            } else {
                None
            })
            .collect();
        info!(
            "Punter {}: Score: {}, Ownes {} edges: {:?}",
            p,
            self.score(p),
            owned_edges.len(),
            owned_edges
        );
    }

    pub fn print_summary(&self) {
        info!("Me: {}", self.me);
        for p in 0..self.punters {
            self.print_punter_summary(p as PunterId);
        }
    }
}
