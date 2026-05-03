default:
  @just --list

CARGO := 'cargo'
CARGO_TARGET := 'x86_64-unknown-linux-musl'
CARGO_BUILD_TYPE_RELEASE := '--release'

release-musl:
	{{CARGO}} build {{CARGO_BUILD_TYPE_RELEASE}} --target={{CARGO_TARGET}} --all-targets
	{{CARGO}} bloat {{CARGO_BUILD_TYPE_RELEASE}} --target={{CARGO_TARGET}}
	{{CARGO}} strip {{CARGO_BUILD_TYPE_RELEASE}} --target={{CARGO_TARGET}} --bin=nlserver
	{{CARGO}} strip {{CARGO_BUILD_TYPE_RELEASE}} --target={{CARGO_TARGET}} --bin=applesingle
	{{CARGO}} strip {{CARGO_BUILD_TYPE_RELEASE}} --target={{CARGO_TARGET}} --bin=applesingleread

dev:
	env RUST_LOG=neolith=trace,nlserver=trace systemfd --no-pid -s tcp::'[::]':5500 -s tcp::'[::]':5501 -- watchexec -i /doc -r -- cargo run

dev-layering:
	env RUST_LOG=neolith=trace,nlserver=trace watchexec -i /doc -r -- cargo run --example=layering
