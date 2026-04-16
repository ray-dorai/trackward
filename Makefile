.PHONY: up down build run test

up:
	docker compose up -d

down:
	docker compose down

build:
	cargo build

run: up
	cp -n .env.example .env 2>/dev/null || true
	set -a && . ./.env && set +a && cargo run -p ledger

test:
	cargo test --workspace
