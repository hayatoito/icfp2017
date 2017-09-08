use chrono;
use pbr;
use punter::bot::{self, Bot, BotMaker};
use punter::game::{Game, Strategy};
use punter::prelude::*;
use punter::protocol::*;
use rand::{self, Rng};
use rayon::prelude::*;
use serde_json;
use std;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct BotStat {
    pub point: Vec<usize>,
    pub score: Vec<i64>,
    move_count: u64,
    consumed_time: std::time::Duration,
}

impl fmt::Display for BotStat {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let game_cnt = self.point.len();
        let score_sum: i64 = self.score.iter().sum();
        let point_sum: usize = self.point.iter().sum();
        let nanos = self.consumed_time.as_secs() * 1_000_000_000 + self.consumed_time.subsec_nanos() as u64;
        let avg_mills = nanos / 1_000_000 / self.move_count;
        write!(
            f,
            "score: {:10}, point: {:.1} (= {:3} / {}), time: {:6}ms) ",
            score_sum,
            if game_cnt == 0 {
                0.0
            } else {
                point_sum as f64 / (game_cnt as f64)
            },
            point_sum,
            game_cnt,
            avg_mills,
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
    fn add(&mut self, result: PunterScore) {
        let bot = &mut self.stats.entry(result.bot_name).or_insert(BotStat {
            point: vec![],
            score: vec![],
            move_count: 0,
            consumed_time: Default::default(),
        });
        bot.point.push(result.point);
        bot.score.push(result.score);
        bot.move_count += result.move_count;
        bot.consumed_time += result.consumed_time;
    }
}

struct Listener {
    bar: pbr::ProgressBar<std::io::Stdout>,
}

impl Listener {
    fn inc(&mut self) {
        self.bar.inc();
    }
}

struct Battle {
    map: Map,
    settings: Settings,
    bots: Vec<Box<Bot>>,
    listener: Option<Arc<Mutex<Listener>>>,
}

impl Battle {
    pub fn new(map: Map, settings: Settings, bots: Vec<Box<Bot>>, listener: Option<Arc<Mutex<Listener>>>) -> Battle {
        Battle {
            map,
            bots,
            settings,
            listener,
        }
    }

    fn run(self) -> PunterResult<Vec<PunterScore>> {
        let Battle {
            map,
            settings,
            bots,
            mut listener,
        } = self;
        let punters = bots.len();

        struct Punter {
            id: PunterId,
            bot: Box<Bot>,
            game: Game,
            #[allow(dead_code)]
            futures: Option<Vec<Future>>,
            last_move: Move,
            state: EncodedGameState,
            score: i64,
            move_count: u64,
            consumed_time: std::time::Duration,
        };

        // Setup phase
        let mut punters = bots.into_iter()
            .enumerate()
            .map(|(punter_id, mut bot)| {
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
                    move_count: 0,
                    consumed_time: std::time::Duration::new(0, 0),
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

            if let Some(ref mut listener) = listener {
                let mut li = listener.lock().unwrap();
                li.inc();
            }

            let now = std::time::Instant::now();
            let mov: Move = match punter.bot.play(gameplay) {
                Ok(rep) => {
                    let state = rep.state();
                    punter.state = state;
                    rep.into()
                }
                Err(_) => {
                    warn!("punter error: {}", punter.id);
                    Move::Pass { pass: Pass { punter: punter.id } }
                }
            };
            punter.move_count += 1;
            punter.consumed_time += now.elapsed();

            info!("move: {:?}", mov);
            punter.game.apply_move(mov.clone());
            punter.last_move = mov.clone();
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

        if std::env::var("MY_ICFP2017_RECORD_BATTLE").is_ok() {
            VisGraph::new(map, moves).write_vis_graph_json();
        }

        let mut scores = punters.iter().map(|p| -p.score).collect::<Vec<i64>>();
        scores.sort();
        Ok(
            punters
                .iter()
                .map(|p| {
                    PunterScore {
                        bot_name: p.bot.name(),
                        point: punters.len() - scores.binary_search(&-p.score).unwrap(),
                        score: p.score,
                        move_count: p.move_count,
                        consumed_time: p.consumed_time,
                    }
                })
                .collect(),
        )
    }
}

struct PunterScore {
    bot_name: String,
    point: usize,
    score: i64,
    move_count: u64,
    consumed_time: std::time::Duration,
}

struct Arena {
    bot_makers: Vec<BotMaker>,
    maps: Vec<PathBuf>,
    games_per_map: usize,
}

impl Arena {
    pub fn run(&self) {

        let maps = self.maps.iter().map(|m| read_map(m)).collect::<Vec<_>>();
        let total_turns = maps.iter().map(|m| m.rivers.len()).sum::<usize>() * self.games_per_map;

        let listener = Arc::new(Mutex::new(
            Listener { bar: pbr::ProgressBar::new(total_turns as u64) },
        ));
        let settings: Settings = Default::default();

        self.maps.par_iter().for_each(|map_path| {
            let map = read_map(map_path);

            let results = (0..self.games_per_map)
                .collect::<Vec<_>>()
                .par_iter()
                .flat_map(|_| {
                    let mut bots = self.bot_makers
                        .iter()
                        .map(|b| b.make())
                        .collect::<Vec<Box<Bot>>>();
                    let mut rng = rand::thread_rng();
                    rng.shuffle(&mut bots);
                    let battle = Battle::new(map.clone(), settings.clone(), bots, Some(listener.clone()));
                    battle.run().expect("fails")

                })
                .collect::<Vec<PunterScore>>();
            let mut stats = ArenaStats::new();
            for r in results {
                stats.add(r);
            }
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            writeln!(
                &mut handle,
                "map: {} (cities: {}, rivers: {})",
                map_path.file_stem().unwrap().to_str().unwrap(),
                map.sites.len(),
                map.rivers.len()
            ).unwrap();
            writeln!(&mut handle, "{}", stats).unwrap();
        });
    }
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

pub fn sample_battle(builtin_map_name: &str) -> ArenaStats {
    let map = read_map(&builtin_map_path(builtin_map_name));
    let settings: Settings = Default::default();
    let bots: Vec<Box<Bot>> = vec![
        Box::new(bot::InternalBot::new(Strategy::EdgeWeight)),
        Box::new(bot::InternalBot::new(Strategy::EdgeWeight)),
    ];

    let mut stats = ArenaStats::new();
    for r in Battle::new(map, settings, bots, None).run().unwrap() {
        stats.add(r);
    }
    stats
}

#[test]
fn regression_test() {
    let stats = sample_battle("lambda.json");
    let s = &stats.stats["EdgeWeight"];
    assert_eq!(s.score, [2504, 2036]);

    let stats = sample_battle("circle.json");
    let s = &stats.stats["EdgeWeight"];
    assert_eq!(s.score, [510, 507]);

    let stats = sample_battle("tube.json");
    let s = &stats.stats["EdgeWeight"];
    assert_eq!(s.score, [89044, 95786]);
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
    let arena = Arena {
        bot_makers: vec![
            BotMaker::Internal(Strategy::Stupid),
            BotMaker::Internal(Strategy::EdgeWeight),
        ],
        maps,
        games_per_map: 8,
    };
    arena.run();
    Ok(())
}

pub fn arena_run<P: AsRef<Path>>(bot_programs: Vec<P>) -> PunterResult<()> {
    let bot_makers = bot_programs
        .into_iter()
        .map(|p| BotMaker::Offline(p.as_ref().to_owned()))
        .collect();

    let maps = [
        // // "sample.json",
        // "lambda.json",
        // "Sierpinski-triangle.json",
        // "circle.json",
        // "randomMedium.json",
        // "randomSparse.json",
        // "tube.json",
        "oxford-10000.json",
        "oxford-center-sparse.json",
        "oxford.json",
        "edinburgh-sparse.json",
        // "boston-sparse.json",
        "nara-sparse.json",
        "van-city-sparse.json",
        "gothenburg-sparse.json",
        // "edinburgh-10000.json",
        // "icfp-coauthors-pj.json",
        // "junction.json",
    ].iter()
        .map(|p| builtin_map_path(p))
        .collect();

    let arena = Arena {
        bot_makers,
        maps,
        games_per_map: 8,
    };
    arena.run();
    Ok(())
}

pub fn single_match<P: AsRef<Path>>(bot_programs: Vec<P>, map_path: P, games: usize) -> PunterResult<()> {
    let bot_makers = bot_programs
        .into_iter()
        .map(|p| BotMaker::Offline(p.as_ref().to_owned()))
        .collect();

    let arena = Arena {
        bot_makers,
        maps: vec![map_path.as_ref().to_owned()],
        games_per_map: games,
    };
    arena.run();
    Ok(())
}
