all: release

prereq:
	curl https://sh.rustup.rs -sSf | sh -s -- --no-modify-path -y
	rustup update
	rustup override set nightly

debug:
	cargo build

release:
	cargo build --release

run-debug:
	cargo run

run-release:
	cargo run --release

package:
	mkdir -p ~/tmp/staging
	rsync -a . ~/tmp/staging
	rm -rf ~/tmp/staging/target/debug
	rm -rf ~/tmp/staging/task
	rm -rf ~/tmp/staging/visualizer
	rm -rf ~/tmp/staging/.git
	cd ~/tmp/staging && tar cvzf ~/tmp/icfp-6dd36dd7-ff7c-4dfb-97fc-c9388a8132bf.tar.gz .
	md5sum ~/tmp/icfp-6dd36dd7-ff7c-4dfb-97fc-c9388a8132bf.tar.gz


ssh:
	echo "vm's setting: host:2222 -> guest:22"
	echo "user: punter"
	echo "password: icfp2017"
	ssh -p 2222 -l punter localhost


rsync:
	: echo on vm: apt-get install rsync
	rsync -avz --progress --delete -e 'ssh -p 2222' ~/src/icfp2017 "punter@localhost:src"
