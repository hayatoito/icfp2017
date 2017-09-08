extern crate icfp2017;

#[macro_use]
extern crate log;
extern crate env_logger;
extern crate loggerv;
extern crate clap;

use clap::{App, Arg, SubCommand};
use icfp2017::punter::arena;
use icfp2017::punter::play;

fn build_cli() -> App<'static, 'static> {
    App::new("icfp2017")
        .version("1.0")
        .arg(Arg::with_name("v").short("v").multiple(true).help(
            "Sets the level of verbosity",
        ))
        .subcommand(SubCommand::with_name("internal-arena"))
        .subcommand(SubCommand::with_name("online").arg(
            Arg::with_name("port").takes_value(true).required(true),
        ))
        .subcommand(SubCommand::with_name("arena").arg(
            Arg::with_name("bot").multiple(true),
        ))
        .subcommand(
            SubCommand::with_name("single-match")
                .arg(
                    Arg::with_name("map")
                        .short("m")
                        .long("map")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("games")
                        .short("g")
                        .long("games")
                        .default_value("1"),
                )
                .arg(Arg::with_name("bot").multiple(true)),
        )
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
    if let Some(_) = matches.subcommand_matches("internal-arena") {
        debug!(">>> internal-arena_run");
        arena::internal_arena_run().expect("internal-arena failes");
    } else if let Some(sub) = matches.subcommand_matches("arena") {
        arena::arena_run(sub.values_of("bot").unwrap().collect::<Vec<_>>()).expect("offline_arena_run fails");
    } else if let Some(sub) = matches.subcommand_matches("single-match") {
        arena::single_match(
            sub.values_of("bot").unwrap().collect::<Vec<_>>(),
            sub.value_of("map").unwrap(),
            sub.value_of("games").unwrap().parse().unwrap(),
        ).expect("single-mach fails");
    } else if let Some(sub) = matches.subcommand_matches("online") {
        debug!(">>> online_run");
        let game = play::online_run(&format!(
            "{}:{}",
            "punter.inf.ed.ac.uk",
            sub.value_of("port").unwrap()
        )).expect("game fails");
        game.print_summary();
    } else {
        debug!(">>> offline_run");
        play::offline_run().expect("offline_run fails");
    }
    debug!("Bye");
}
