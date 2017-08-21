#![feature(test)]
extern crate test;

extern crate icfp2017;

use icfp2017::punter::arena;
use test::Bencher;

#[bench]
fn sample_battle(b: &mut Bencher) {
    b.iter(|| arena::sample_battle());
}
