install:
	cargo build --release && install -m 755 target/release/rostrum ~/.local/bin
