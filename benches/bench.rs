#![feature(test)]
extern crate test;

extern crate icfp2017;

use icfp2017::punter::arena;
use test::Bencher;

#[bench]
fn circle_battle(b: &mut Bencher) {
    b.iter(|| arena::sample_battle("circle.json"));
}

#[bench]
fn lambda_battle(b: &mut Bencher) {
    b.iter(|| arena::sample_battle("lambda.json"));
}

#[bench]
fn tube_battle(b: &mut Bencher) {
    b.iter(|| arena::sample_battle("tube.json"));
}
