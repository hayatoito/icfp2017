use chrono;
use punter::bot::{self, Bot, InternalBot, OfflineBot};
use punter::game::{Game, Strategy};
use punter::prelude::*;
use punter::protocol::*;
use rand::{self, Rng};
use serde_json;
use std;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct BotStat {
    pub point: Vec<usize>,
    pub score: Vec<i64>,
}

impl fmt::Display for BotStat {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let game_cnt = self.point.len();
        let score_sum: i64 = self.score.iter().sum();
        let point_sum: usize = self.point.iter().sum();
        write!(
            f,
            "score: {} (= {} / {}), point: {:.1} (= {} / {}) ",
            if game_cnt == 0 {
                0
            } else {
                score_sum / (game_cnt as i64)
            },
            score_sum,
            game_cnt,
            if game_cnt == 0 {
                0.0
            } else {
                point_sum as f64 / (game_cnt as f64)
            },
            point_sum,
            game_cnt,
        )
    }
}

#[derive(Debug)]
pub struct ArenaStats {
    pub stats: BTreeMap<String, BotStat>,
}

impl fmt::Display for ArenaStats {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (name, bot_stat) in self.stats.iter() {
            writeln!(f, "{:>32}, {}", name, bot_stat)?;
        }
        Ok(())
    }
}

impl ArenaStats {
    fn new() -> Self {
        ArenaStats { stats: Default::default() }
    }
}

impl ArenaStats {
    fn add(&mut self, bot_name: String, point: usize, score: i64) {
        let bot = &mut self.stats.entry(bot_name).or_insert(BotStat {
            point: vec![],
            score: vec![],
        });
        bot.point.push(point);
        bot.score.push(score);
    }
}

struct Battle<'a> {
    map: Map,
    bots: &'a mut [Box<Bot>],
    settings: Settings,
}

impl<'a> Battle<'a> {
    pub fn new(map: Map, bots: &mut [Box<Bot>], settings: Settings) -> Battle {
        Battle {
            map,
            bots,
            settings,
        }
    }

    fn run(self, stats: &mut ArenaStats) -> PunterResult<()> {
        let Battle {
            map,
            bots,
            settings,
        } = self;
        let punters = bots.len();

        struct Punter<'a> {
            id: PunterId,
            bot: &'a mut Box<Bot>,
            game: Game,
            #[allow(dead_code)]
            futures: Option<Vec<Future>>,
            last_move: Move,
            state: EncodedGameState,
            score: i64,
        };

        // Setup phase
        let mut punters = bots.iter_mut()
            .enumerate()
            .map(|(punter_id, bot)| {
                let setup = SetupSP {
                    punter: punter_id,
                    punters: punters,
                    map: map.clone(),
                    settings: Some(settings.clone()),
                };
                let rep = bot.setup(setup.clone())?;
                Ok(Punter {
                    id: punter_id,
                    bot: bot,
                    game: setup.into(),
                    futures: rep.futures,
                    last_move: Move::Pass { pass: Pass { punter: punter_id } },
                    state: rep.state,
                    score: 0,
                })
            })
            .collect::<Result<Vec<Punter>, PunterError>>()?;

        let mut moves = vec![];

        // Gameplay phase
        let turns = map.rivers.len();
        for i in (0..punters.len()).cycle().take(turns) {
            let last_moves = punters.iter().map(|p| p.last_move.clone()).collect();
            let punter = &mut punters[i];
            let gameplay = OfflineGamePlaySP {
                moves: Moves { moves: last_moves },
                state: punter.state.clone(),
            };

            let rep = punter.bot.play(gameplay)?;

            let state = rep.state();
            let mov: Move = rep.into();
            info!("move: {:?}", mov);

            punter.game.apply_move(mov.clone());
            punter.last_move = mov.clone();
            punter.state = state;
            moves.push(mov);
        }

        // Scoring phase
        for p in punters.iter_mut() {
            p.score = p.game.score(p.game.me);
        }

        for i in 0..punters.len() {
            let punter_index = (i + turns) % punters.len();

            let scores = Scores {
                moves: punters.iter().map(|p| p.last_move.clone()).collect(),
                scores: punters
                    .iter()
                    .map(|p| {
                        Score {
                            punter: p.id,
                            score: p.score,
                        }
                    })
                    .collect(),
            };

            let punter = &mut punters[punter_index];
            punter.bot.stop(OfflineScoringSP {
                stop: scores,
                state: punter.state.clone(),
            })?;
            punter.last_move = Move::Pass { pass: Pass { punter: punter_index } };
        }

        let mut scores = punters.iter().map(|p| -p.score).collect::<Vec<i64>>();
        scores.sort();
        for p in &punters {
            stats.add(
                p.bot.name(),
                punters.len() - scores.binary_search(&-p.score).unwrap(),
                p.score,
            );
        }
        VisGraph::new(map, moves).write_vis_graph_json();
        Ok(())
    }
}

struct Arena {
    bots: Vec<Box<Bot>>,
    maps: Vec<PathBuf>,
    games_per_map: usize,
}

impl Arena {
    pub fn run(&mut self) {
        for map_path in &self.maps {
            let map = read_map(map_path);
            let settings: Settings = Default::default();
            let mut stats = ArenaStats::new();
            for _ in 0..self.games_per_map {
                let mut rng = rand::thread_rng();
                rng.shuffle(&mut self.bots);
                let battle = Battle::new(map.clone(), &mut self.bots, settings.clone());
                battle.run(&mut stats).expect("fails");
            }
            println!("map: {}", map_path.file_stem().unwrap().to_str().unwrap());
            println!("{}", stats);
        }
    }
}

#[test]
fn parse_sample_map() {
    read_map(&builtin_map_path("sample.json"));
}

fn builtin_map_path(map_name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("task/maps");
    path.push(map_name);
    path
}

fn read_map(path: &Path) -> Map {
    let mut f = fs::File::open(&path).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}

#[derive(Debug, Serialize)]
struct VisGraph {
    map: Map,
    moves: Vec<Move>,
}

impl VisGraph {
    fn new(map: Map, moves: Vec<Move>) -> Self {
        VisGraph { map, moves }
    }

    fn write_vis_graph_json(&self) {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let local = chrono::Local::now();
        path.push("visualizer/data");
        path.push(local.format("%Y-%m-%d-%H-%M-%S-%f.json").to_string());

        let json = serde_json::to_string(self).unwrap();
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

pub fn sample_battle() -> ArenaStats {
    let map = read_map(&builtin_map_path("circle.json"));
    let settings: Settings = Default::default();
    let mut bots: Vec<Box<Bot>> = vec![
        Box::new(bot::InternalBot::new(Strategy::EdgeWeight)),
        Box::new(bot::InternalBot::new(Strategy::EdgeWeight)),
    ];

    let mut stats = ArenaStats::new();
    Battle::new(map, &mut bots, settings)
        .run(&mut stats)
        .unwrap();
    stats
}

#[test]
fn regression_test() {
    let stats = sample_battle();
    let s = &stats.stats["EdgeWeight"];
    assert_eq!(s.score, [510, 507]);
}

pub fn internal_arena_run() -> PunterResult<()> {
    let maps = [
        "lambda.json",
        "Sierpinski-triangle.json",
        "circle.json",
        "randomMedium.json",
        "randomSparse.json",
    ].iter()
        .map(|p| builtin_map_path(p))
        .collect();
    let mut arena = Arena {
        bots: vec![
            Box::new(InternalBot::new(Strategy::Stupid)),
            Box::new(InternalBot::new(Strategy::EdgeWeight)),
        ],
        maps,
        games_per_map: 8,
    };
    arena.run();
    Ok(())
}

pub fn arena_run<P: AsRef<Path>>(bot_programs: Vec<P>) -> PunterResult<()> {
    let bots = bot_programs
        .into_iter()
        .map(|p| {
            Box::new(OfflineBot::new(p.as_ref().to_owned())) as Box<Bot>
        })
        .collect();

    let maps = [
        // "sample.json",
        "lambda.json",
        "Sierpinski-triangle.json",
        "circle.json",
        "randomMedium.json",
        "randomSparse.json",
        // "tube.json",
        // "oxford-center-sparse.json",
        // "oxford.json",
        // "edinburgh-sparse.json",
        // "boston-sparse.json",
        // "nara-sparse.json",
        // "van-city-sparse.json",
        // "gothenburg-sparse.json",
    ].iter()
        .map(|p| builtin_map_path(p))
        .collect();

    let mut arena = Arena {
        bots,
        maps,
        games_per_map: 8,
    };
    arena.run();
    Ok(())
}

pub fn single_match<P: AsRef<Path>>(bot_programs: Vec<P>, map_path: P, games: usize) -> PunterResult<()> {
    let bots = bot_programs
        .into_iter()
        .map(|p| {
            Box::new(OfflineBot::new(p.as_ref().to_owned())) as Box<Bot>
        })
        .collect();

    let mut arena = Arena {
        bots,
        maps: vec![map_path.as_ref().to_owned()],
        games_per_map: games,
    };
    arena.run();
    Ok(())
}
